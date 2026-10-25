//! UART RX interrupt path (PLIC-driven input).
//!
//! Before this module the console was LSR-poll only: `getc()` asked the
//! NS16550A whether a byte sat in the FIFO, and every blocking reader spun
//! through `sched_yield` until a timer tick happened to notice input. Two
//! consequences: keystrokes waited up to one tick (~10 ms) when the hart was
//! idle, and a burst longer than the 16-byte FIFO was lost while a process
//! was busy in user space.
//!
//! Now the PLIC delivers the UART line as IRQ 10, the handler drains the
//! FIFO into a 4 KiB software ring, and readers pop from the ring. The
//! interrupt wakes the per-hart idle loop out of `wfi`, so a blocked
//! `read(0)` is serviced as soon as the byte arrives instead of on the next
//! timer tick. Polling is kept as a fallback: `getc()` still reads the FIFO
//! directly when the ring is empty, which covers the window before
//! `early_init` registers the handler and any platform without PLIC.
//!
//! Concurrency: the producer is the PLIC handler on hart 0 (the only hart
//! the UART IRQ is enabled for), consumers are console readers on any hart.
//! `RING` is therefore guarded by `LOCK`. Taking a `SpinLock` here is safe
//! under the crate-wide invariant (`crate::sync`): kernel code always runs
//! with `sstatus.SIE` clear, so neither the handler nor a reader can be
//! preempted while holding it.

use crate::drivers::uart;
use crate::sync::SpinLock;

/// Ring capacity (power of two so the index math is a mask).
pub const RX_RING_SIZE: usize = 4096;
const MASK: usize = RX_RING_SIZE - 1;
/// Upper bound on bytes moved per interrupt: one FIFO's worth plus a burst
/// that arrived while the hart was in `wfi`. Bounded so a stalled reader
/// cannot keep this hart inside the handler.
const MAX_BYTES_PER_IRQ: usize = 64;

struct Ring {
    buf: [u8; RX_RING_SIZE],
    head: usize,
    tail: usize,
}

impl Ring {
    const fn new() -> Self {
        Self {
            buf: [0; RX_RING_SIZE],
            head: 0,
            tail: 0,
        }
    }

    /// Returns false when the ring is full (the byte is dropped; the read
    /// side keeps the older data, matching a bounded kernel buffer).
    fn push(&mut self, b: u8) -> bool {
        let next = (self.head + 1) & MASK;
        if next == self.tail {
            return false;
        }
        self.buf[self.head] = b;
        self.head = next;
        true
    }

    fn pop(&mut self) -> Option<u8> {
        if self.head == self.tail {
            return None;
        }
        let b = self.buf[self.tail];
        self.tail = (self.tail + 1) & MASK;
        Some(b)
    }

    fn len(&self) -> usize {
        self.head.wrapping_sub(self.tail) & MASK
    }

    fn is_empty(&self) -> bool {
        self.head == self.tail
    }
}

static LOCK: SpinLock = SpinLock::new();
static mut RING: Ring = Ring::new();
/// Total bytes moved by the handler; lets tests/diagnostics prove the IRQ
/// path (not the poll fallback) delivered input.
static mut G_RX_BYTES: usize = 0;
static mut G_DROPPED: usize = 0;
/// One-shot evidence flag: the console input path really is interrupt
/// driven (not the `getc` FIFO-poll fallback), printed on the first IRQ.
static mut G_FIRST_IRQ: bool = false;

/// Register the PLIC handler and enable the UART "received data available"
/// interrupt. Called once from boot, after `plic::init`/`plic::enable`.
///
/// # Safety
///
/// Single-threaded boot, on the hart owning the PLIC S-mode context (hart
/// 0), with the PLIC base already validated.
pub unsafe fn init() {
    // SAFETY: caller contract above; registration is bounded by MAX_IRQ.
    unsafe {
        crate::drivers::plic::register_handler(crate::arch::regs::PLIC_PRIO_UART, handler);
    }
    uart::enable_rx_irq();
    crate::kinf!(
        "uart",
        "rx irq enabled (irq=%d ring=%d B)",
        onyx_core::fmt::Arg::from(crate::arch::regs::PLIC_PRIO_UART as usize),
        onyx_core::fmt::Arg::from(RX_RING_SIZE)
    );
}

/// PLIC dispatch entry: drain the hardware FIFO into the ring.
fn handler() {
    LOCK.lock();
    let mut moved = 0usize;
    let mut dropped = 0usize;
    // SAFETY: RING is only touched under LOCK (handler and pop/poll paths
    // all take it); the raw pointer avoids a `static mut` reference.
    unsafe {
        let ring = &mut *(&raw mut RING);
        while moved < MAX_BYTES_PER_IRQ {
            let Some(b) = uart::read_fifo_byte() else {
                break;
            };
            if !ring.push(b) {
                dropped += 1;
            }
            moved += 1;
        }
        G_RX_BYTES = G_RX_BYTES.saturating_add(moved - dropped);
        G_DROPPED = G_DROPPED.saturating_add(dropped);
        if moved > 0 && !G_FIRST_IRQ {
            G_FIRST_IRQ = true;
            crate::kinf!(
                "uart",
                "rx irq: %d byte(s) buffered",
                onyx_core::fmt::Arg::from(moved)
            );
        }
    }
    LOCK.unlock();
}

/// Pop the next buffered byte, if any.
pub fn pop() -> Option<u8> {
    LOCK.lock();
    // SAFETY: RING is only touched under LOCK.
    let out = unsafe { (*(&raw mut RING)).pop() };
    LOCK.unlock();
    out
}

/// True when at least one byte is buffered.
pub fn has_data() -> bool {
    LOCK.lock();
    // SAFETY: RING is only touched under LOCK.
    let empty = unsafe { (*(&raw mut RING)).is_empty() };
    LOCK.unlock();
    !empty
}

/// Buffered byte count (FIONREAD / poll accounting).
pub fn available() -> usize {
    LOCK.lock();
    // SAFETY: RING is only touched under LOCK.
    let len = unsafe { (*(&raw mut RING)).len() };
    LOCK.unlock();
    len
}

/// Bytes moved by the interrupt handler since boot (diagnostics).
pub fn irq_bytes() -> usize {
    LOCK.lock();
    // SAFETY: counter is written under LOCK by the handler.
    let n = unsafe { G_RX_BYTES };
    LOCK.unlock();
    n
}

/// Bytes dropped because the ring was full (diagnostics).
pub fn dropped() -> usize {
    LOCK.lock();
    // SAFETY: counter is written under LOCK by the handler.
    let n = unsafe { G_DROPPED };
    LOCK.unlock();
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_is_fifo() {
        let mut r = Ring::new();
        assert!(r.is_empty());
        assert!(r.push(b'a'));
        assert!(r.push(b'b'));
        assert_eq!(r.len(), 2);
        assert_eq!(r.pop(), Some(b'a'));
        assert_eq!(r.pop(), Some(b'b'));
        assert_eq!(r.pop(), None);
        assert!(r.is_empty());
    }

    #[test]
    fn ring_rejects_when_full() {
        let mut r = Ring::new();
        // One slot stays free so head == tail can mean empty.
        for _ in 0..RX_RING_SIZE - 1 {
            assert!(r.push(1));
        }
        assert_eq!(r.len(), RX_RING_SIZE - 1);
        assert!(!r.push(2));
        assert_eq!(r.len(), RX_RING_SIZE - 1);
        // Draining one slot makes room again.
        assert_eq!(r.pop(), Some(1));
        assert!(r.push(3));
    }

    #[test]
    fn ring_wraps_across_the_end() {
        let mut r = Ring::new();
        for i in 0..RX_RING_SIZE - 1 {
            assert!(r.push(i as u8));
        }
        for i in 0..RX_RING_SIZE - 1 {
            assert_eq!(r.pop(), Some(i as u8));
        }
        // head/tail are back at the start, but with the wrap the next push
        // must land at index head, not overflow the buffer.
        for i in 0..100u8 {
            assert!(r.push(i));
        }
        for i in 0..100u8 {
            assert_eq!(r.pop(), Some(i));
        }
    }
}

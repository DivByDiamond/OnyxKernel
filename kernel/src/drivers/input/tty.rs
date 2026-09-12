//! Console TTY FIFO for virtio-input keyboard.
//!
//! The OC2R monitor delivers keyboard events via virtio-input (evdev)
//! on virtio-mmio. Raw UART polling therefore never sees monitor typing.
//! This FIFO buffers ASCII bytes produced from `input::Event::Key` and
//! is drained by `console_read` (all line disciplines) and `poll(2)`.
//! The virtio-input side pushes from `input::dispatch`; readers pop
//! from syscall context. Both contexts run with SIE=0 (see sync.rs:16-24)
//! so the SpinLock never contends with an interrupt preemption.

use crate::sync::SpinLock;

const CAP: usize = 256;

static LOCK: SpinLock = SpinLock::new();
static mut BUF: [u8; CAP] = [0; CAP];
static mut HEAD: usize = 0;
static mut TAIL: usize = 0;
static mut LEN: usize = 0;

/// Push one byte into the FIFO. Returns false when full (byte dropped).
pub fn push(b: u8) -> bool {
    LOCK.lock();
    // SAFETY: LEN/BUF/TAIL are kernel-owned statics; access is serialized
    // by LOCK and only from kernel context with SIE=0 (sync invariant).
    unsafe {
        if LEN >= CAP {
            LOCK.unlock();
            return false;
        }
        BUF[TAIL] = b;
        TAIL = (TAIL + 1) % CAP;
        LEN += 1;
    }
    LOCK.unlock();
    true
}

/// Pop one byte from the FIFO. Returns None when empty.
pub fn pop() -> Option<u8> {
    LOCK.lock();
    // SAFETY: same ownership/serialization as in push().
    unsafe {
        if LEN == 0 {
            LOCK.unlock();
            return None;
        }
        let b = BUF[HEAD];
        HEAD = (HEAD + 1) % CAP;
        LEN -= 1;
        LOCK.unlock();
        Some(b)
    }
}

/// Push a short byte sequence into the FIFO, stopping if it becomes full.
pub fn push_slice(data: &[u8]) -> usize {
    let mut n = 0usize;
    for &b in data {
        if !push(b) {
            break;
        }
        n += 1;
    }
    n
}

/// True when at least one byte is queued.
pub fn has_data() -> bool {
    LOCK.lock();
    // SAFETY: LEN is kernel-owned static serialized by LOCK.
    let v = unsafe { LEN != 0 };
    LOCK.unlock();
    v
}

/// Number of bytes queued.
pub fn pending() -> usize {
    LOCK.lock();
    // SAFETY: LEN is kernel-owned static serialized by LOCK.
    let v = unsafe { LEN };
    LOCK.unlock();
    v
}

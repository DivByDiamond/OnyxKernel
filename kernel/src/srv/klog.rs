//! klog — formatted logging via UART.
use crate::drivers::uart;
use crate::sync::SpinLock;
use core::panic::PanicInfo;
use core::sync::atomic::{AtomicI32, AtomicU8, AtomicU32, Ordering};
use onyx_core::fmt::{Arg, Write, vformat};

/// Serializes UART output across harts. Without this, concurrent kinf!/
/// kerr! calls from different harts interleave byte-by-byte on the shared
/// UART MMIO register, producing garbled log lines that are unreadable
/// exactly when they matter most (a crash on one hart while another is
/// still printing). Diagnostic instrumentation for the SMP crash
/// investigation (todo.md, "Отдельный SMP-краш под -smp 2"), kept as a
/// permanent fix since the underlying data race is real regardless.
///
/// Reentrant by hart: a fault that occurs while THIS hart is already mid-
/// print (e.g. a nested trap firing while formatting a log line) must still
/// be able to report itself instead of deadlocking the hart against its own
/// held lock — the plain `SpinLock` is not reentrant, so ownership is
/// tracked separately here and re-entry from the same hart is a no-op.
static UART_LOCK_OWNER: AtomicI32 = AtomicI32::new(-1);
static UART_LOCK_DEPTH: AtomicU32 = AtomicU32::new(0);
pub(crate) struct UartLock;
pub(crate) static UART_LOCK: UartLock = UartLock;
impl UartLock {
    pub(crate) fn lock(&self) {
        let hart = crate::proc::process::hart_id() as i32;
        if UART_LOCK_OWNER.load(Ordering::Acquire) == hart {
            UART_LOCK_DEPTH.fetch_add(1, Ordering::Relaxed);
            return;
        }
        RAW_UART_LOCK.lock();
        UART_LOCK_OWNER.store(hart, Ordering::Release);
        UART_LOCK_DEPTH.store(1, Ordering::Relaxed);
    }
    pub(crate) fn unlock(&self) {
        if UART_LOCK_DEPTH.fetch_sub(1, Ordering::Relaxed) == 1 {
            UART_LOCK_OWNER.store(-1, Ordering::Release);
            RAW_UART_LOCK.unlock();
        }
    }
}
static RAW_UART_LOCK: SpinLock = SpinLock::new();

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Err = 0,
    Wrn = 1,
    Inf = 2,
    Dbg = 3,
}
impl Level {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Dbg => "DBG",
            Self::Inf => "INF",
            Self::Wrn => "WRN",
            Self::Err => "ERR",
        }
    }
}

/// Maximum level still printed. Defaults to `Info` (the historical
/// behaviour: info, warnings and errors go out, debug chatter does not).
static MAX_LEVEL: AtomicU8 = AtomicU8::new(Level::Inf as u8);

pub fn set_max_level(level: Level) {
    MAX_LEVEL.store(level as u8, Ordering::Relaxed);
}

#[inline]
pub fn enabled(level: Level) -> bool {
    (level as u8) <= MAX_LEVEL.load(Ordering::Relaxed)
}

struct UartWriter;
impl Write for UartWriter {
    fn write_str(&mut self, s: &str) {
        for &b in s.as_bytes() {
            if b == b'\n' {
                uart::putc(b'\r');
            }
            uart::putc(b);
        }
    }
    fn write_char(&mut self, c: u8) {
        if c == b'\n' {
            uart::putc(b'\r');
        }
        uart::putc(c);
    }
}

pub fn debug_mark(c: u8) {
    // Host-test guard: boot-progress marks write UART MMIO (0x1000_0000),
    // which would fault the host test process. Compile them out under
    // cfg(test) so boot-path code (e.g. fdt::init_from rejection marks)
    // stays unit-testable.
    #[cfg(test)]
    {
        let _ = c;
    }
    #[cfg(not(test))]
    {
        uart::putc(b'[');
        uart::putc(c);
        uart::putc(b']');
    }
}

pub fn puts(s: &str) {
    for &b in s.as_bytes() {
        if b == b'\n' {
            uart::putc(b'\r');
        }
        uart::putc(b);
    }
}
pub fn putc(c: u8) {
    if c == b'\n' {
        uart::putc(b'\r');
    }
    uart::putc(c);
}

pub fn emit(level: Level, tag: &str, fmt: &str, args: &[Arg]) {
    if !enabled(level) {
        return;
    }
    UART_LOCK.lock();
    let mut w = UartWriter;
    w.write_char(b'[');
    w.write_str(level.as_str());
    w.write_char(b']');
    w.write_char(b' ');
    w.write_str(tag);
    w.write_str(": ");
    vformat(&mut w, fmt, args);
    w.write_char(b'\n');
    UART_LOCK.unlock();
}

// The enabled() guard sits in the macro (not inside emit) so a filtered-out
// call also skips building the Arg slice and the vformat work in emit.
#[macro_export]
macro_rules! kdbg { ($tag:expr, $fmt:expr $(, $arg:expr)* $(,)?) => { if $crate::srv::klog::enabled($crate::srv::klog::Level::Dbg) { $crate::srv::klog::emit($crate::srv::klog::Level::Dbg, $tag, $fmt, &[$($arg),*]) } }; }
#[macro_export]
macro_rules! kinf { ($tag:expr, $fmt:expr $(, $arg:expr)* $(,)?) => { if $crate::srv::klog::enabled($crate::srv::klog::Level::Inf) { $crate::srv::klog::emit($crate::srv::klog::Level::Inf, $tag, $fmt, &[$($arg),*]) } }; }
#[macro_export]
macro_rules! kwrn { ($tag:expr, $fmt:expr $(, $arg:expr)* $(,)?) => { if $crate::srv::klog::enabled($crate::srv::klog::Level::Wrn) { $crate::srv::klog::emit($crate::srv::klog::Level::Wrn, $tag, $fmt, &[$($arg),*]) } }; }
#[macro_export]
macro_rules! kerr { ($tag:expr, $fmt:expr $(, $arg:expr)* $(,)?) => { if $crate::srv::klog::enabled($crate::srv::klog::Level::Err) { $crate::srv::klog::emit($crate::srv::klog::Level::Err, $tag, $fmt, &[$($arg),*]) } }; }
// Panic output is intentionally unconditional.
#[macro_export]
macro_rules! kpanic { ($tag:expr, $fmt:expr $(, $arg:expr)* $(,)?) => { { $crate::srv::klog::emit($crate::srv::klog::Level::Err, $tag, $fmt, &[$($arg),*]); $crate::srv::klog::halt() } }; }

pub use onyx_core::fmt::Arg as FmtArg;

pub struct PanicWriter;
impl onyx_core::fmt::Write for PanicWriter {
    fn write_str(&mut self, s: &str) {
        for &b in s.as_bytes() {
            if b == b'\n' {
                uart::putc(b'\r');
            }
            uart::putc(b);
        }
    }
}

fn delay_loops(n: u64) {
    for _ in 0..n {
        // SAFETY: bare `nop` asm -- no memory access and no registers declared as clobbered.
        unsafe { core::arch::asm!("nop") }
    }
}

pub fn panic_handler(info: &PanicInfo) -> ! {
    // Panics bypassed UART_LOCK entirely (PanicWriter writes uart::putc
    // directly), so a panic on one hart while another hart was mid-emit()
    // interleaved byte-by-byte with it — the exact garbling that made these
    // SMP crash logs unreadable. Never returns, so no matching unlock is
    // needed; a panic while THIS hart already holds the lock (inside its
    // own emit() call) would self-deadlock, but no kerr!/kinf! call in this
    // codebase panics before returning, so that path is not reachable.
    UART_LOCK.lock();
    let mut w = PanicWriter;
    w.write_str("\n\n*** KERNEL PANIC ***\n");
    if let Some(loc) = info.location() {
        let args: &[Arg] = &[
            Arg::from(loc.file()),
            Arg::from(loc.line()),
            Arg::from(loc.column()),
        ];
        vformat(&mut w, "  at %s:%d:%d\n", args);
    }
    // TODO(dead-code): `core::panic::PanicInfo::payload` is deprecated
    // upstream (pending PanicHookInfo migration); the &str downcast remains
    // the only no_std-compatible way to read a string panic message.
    // Revisit on a toolchain bump. 2026-08-24
    #[expect(deprecated)]
    if let Some(msg) = info.payload().downcast_ref::<&str>() {
        w.write_str("  msg: ");
        w.write_str(msg);
        w.write_char(b'\n');
    }
    // SAFETY: panic path -- kdump only reads CSRs and process state (volatile reads) and prints; no allocation.
    unsafe {
        crate::srv::kdump::kdump();
    }
    w.write_str("\n  Active processes:\n");
    crate::proc::dump_all(&mut w);
    w.write_str("\n  Rebooting in 3 seconds...\n");
    delay_loops(300_000_000);
    // SAFETY: 0x100000 is the fixed QEMU-virt sifive_test finisher MMIO register; the volatile
    // word write of 0x5555 requests shutdown and bypasses compiler reordering.
    unsafe {
        let finisher = 0x100000usize as *mut u32;
        core::ptr::write_volatile(finisher, 0x5555);
    }
    halt();
}

pub fn halt() -> ! {
    // SAFETY: S-mode CSR write clears SIE (stops same-hart timer preemption) then parks in a wfi loop; no memory access.
    unsafe {
        crate::arch::csr::clear_sstatus(crate::arch::regs::SSTATUS_SIE);
        loop {
            crate::arch::csr::wfi();
        }
    }
}

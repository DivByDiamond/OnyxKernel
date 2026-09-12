//! UART serialization lock — reentrant per-hart spinlock guarding UART output.
use crate::sync::SpinLock;
use core::sync::atomic::{AtomicI32, AtomicU32, Ordering};

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
    /// Non-blocking acquire for interrupt contexts (console frame
    /// presenter): returns false instead of spinning when another hart is
    /// mid-write. Reentrant on the owning hart like [`lock`].
    pub(crate) fn try_lock(&self) -> bool {
        let hart = crate::proc::process::hart_id() as i32;
        if UART_LOCK_OWNER.load(Ordering::Acquire) == hart {
            UART_LOCK_DEPTH.fetch_add(1, Ordering::Relaxed);
            return true;
        }
        if RAW_UART_LOCK.try_lock() {
            UART_LOCK_OWNER.store(hart, Ordering::Release);
            UART_LOCK_DEPTH.store(1, Ordering::Relaxed);
            true
        } else {
            false
        }
    }
    pub(crate) fn unlock(&self) {
        if UART_LOCK_DEPTH.fetch_sub(1, Ordering::Relaxed) == 1 {
            UART_LOCK_OWNER.store(-1, Ordering::Release);
            RAW_UART_LOCK.unlock();
        }
    }
}
static RAW_UART_LOCK: SpinLock = SpinLock::new();

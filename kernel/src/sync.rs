//! Kernel synchronisation primitives.
//!
//! Single source of truth for spinlocks (audit fix): five independent
//! implementations previously existed (pmm, heap, vmm, scheduler rq_lock,
//! proc_list_lock) with inconsistent backoff behaviour — the heap version
//! had none at all and could hammer a contended cache line indefinitely.

use core::{
    hint::spin_loop,
    sync::atomic::{AtomicBool, Ordering},
};

/// Global spinlock used by every kernel subsystem (`pmm`, `heap`, `vmm`,
/// scheduler runqueues, process list, IPC channels).
///
/// # Interrupt invariant (CRITICAL)
///
/// Must only be taken with interrupts disabled in kernel context. This is
/// enforced by `trap_return` (`arch/asm/trap_asm.rs`), which
/// unconditionally clears `sstatus.SIE` before returning to ANY context —
/// user or idle — so a timer tick can never preempt a hart while it holds
/// one of these locks. Do NOT re-enable SIE in kernel context around code
/// that takes a `SpinLock`; doing so reintroduces the classic
/// preempted-while-holding-spinlock deadlock.
///
/// The only context that runs with SIE set is the idle loop
/// (`proc/scheduler/idle.rs`), which re-enables SIE immediately before
/// `wfi` while provably holding no lock (its body takes none, and any
/// interrupt it takes re-enters the SIE = 0 world before running kernel
/// code). Return to user mode relies on the hardware SPIE → SIE mechanism
/// in `sret`, not on this loop.
pub struct SpinLock {
    locked: AtomicBool,
}

// SAFETY: the lock state is a plain atomic flag; guarded data lives outside
// the lock (callers access `static mut` state while holding it), so `Sync`
// is sound for the lock itself.
unsafe impl Sync for SpinLock {}

impl Default for SpinLock {
    fn default() -> Self {
        Self::new()
    }
}

impl SpinLock {
    pub const fn new() -> Self {
        SpinLock {
            locked: AtomicBool::new(false),
        }
    }

    /// Acquire the lock, spinning with cpu-relax backoff until it is free.
    ///
    /// See the type-level interrupt invariant above: never acquire from a
    /// context running with SIE set, and never yield/sleep while held.
    pub fn lock(&self) {
        while self.locked.swap(true, Ordering::Acquire) {
            // Two-phase backoff: poll Relaxed (no RMW traffic on the cache
            // line) with spin_loop hints, retrying the swap periodically in
            // case the poll loop raced past an unlock.
            let mut spins = 0u32;
            while self.locked.load(Ordering::Relaxed) {
                spin_loop();
                spins += 1;
                if spins >= 64 {
                    break;
                }
            }
        }
    }

    /// Try to acquire the lock without spinning. Returns `true` on success.
    pub fn try_lock(&self) -> bool {
        !self.locked.swap(true, Ordering::Acquire)
    }

    /// Release the lock. Release ordering makes all writes done while held
    /// visible to the next acquirer.
    pub fn unlock(&self) {
        self.locked.store(false, Ordering::Release);
    }
}

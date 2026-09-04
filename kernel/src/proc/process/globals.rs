use core::{
    ptr,
    sync::atomic::{AtomicBool, AtomicU32, Ordering},
};

use super::types::{PROC_PID_INIT, Proc};
use crate::{
    arch::{smp, trap_frame::TrapFrame},
    sync::SpinLock,
};

pub const MAX_HARTS: usize = smp::MAX_HARTS;

pub static mut G_ALL_PROCS: *mut Proc = ptr::null_mut();

pub static mut G_HART_CURRENT: [*mut Proc; MAX_HARTS] = [ptr::null_mut(); MAX_HARTS];

pub static mut G_HART_IDLE_TF: [TrapFrame; MAX_HARTS] = [TrapFrame::zero(); MAX_HARTS];

/// Root-cause fix (SMP crash, todo.md "Отдельный SMP-краш под -smp 2"):
/// tracks, per hart, whether `G_HART_IDLE_TF[hart]` holds a real captured
/// idle context yet. Secondary harts always populate theirs before any
/// process can run on them (`sched_enter_idle` is their boot entry, so their
/// very first trap out of `wfi` seeds it). The BOOT hart (0) never calls
/// `sched_enter_idle` — `srv::main::init::launch` drops straight into
/// `enter_user(1)` — so it can go its entire life never voluntarily idling,
/// right up until the moment every runnable process has migrated to another
/// hart (work-stealing) and its own last process exits. At that point
/// `sched_yield`'s "current Exited, nothing to run" path tried to *resume*
/// `G_HART_IDLE_TF[0]`, which was still `TrapFrame::zero()` — sepc=0, sp=0
/// — an immediate crash on switch-back. `sched_yield` checks this flag and,
/// when unset, jumps into `sched_enter_idle()` fresh instead of resuming a
/// frame that was never captured.
pub static G_HART_IDLE_TF_VALID: [AtomicBool; MAX_HARTS] =
    [const { AtomicBool::new(false) }; MAX_HARTS];

pub static G_NEED_RESCHED: [AtomicBool; MAX_HARTS] = [const { AtomicBool::new(false) }; MAX_HARTS];

pub static mut G_CURRENT: *mut Proc = ptr::null_mut();

pub static G_NEXT_PID: AtomicU32 = AtomicU32::new(PROC_PID_INIT + 1);

/// Global process-list spinlock (Bug #16 fix). All mutations and iterations
/// of `G_ALL_PROCS` (the singly-linked list of all Proc nodes) must hold
/// this lock, preventing the race where two harts simultaneously reap the
/// same exited child via `wait()` / `waitpid()` and double-`kfree` the Proc
/// node, or where one hart iterates the list while another is removing a
/// node (orphaned/duplicated processes, UAF).
///
/// Lock ordering: PROC_LIST_LOCK is outermost. Never acquire it while
/// already holding an rq_lock — acquire PROC_LIST_LOCK first, then
/// rq_lock inside if needed.
///
/// Uses the shared `SpinLock` primitive (interrupt invariant: only take with
/// interrupts disabled in kernel context — see `crate::sync::SpinLock`).
pub static G_PROC_LIST_LOCK: SpinLock = SpinLock::new();

#[inline]
pub fn proc_list_lock() {
    G_PROC_LIST_LOCK.lock();
}

#[inline]
pub fn proc_list_unlock() {
    G_PROC_LIST_LOCK.unlock();
}

#[inline]
pub fn hart_id() -> usize {
    #[cfg(not(test))]
    {
        let id: usize;
        // SAFETY: inline asm only reads the tp register (per-hart thread
        // pointer set by boot); no side effects, single `out` operand.
        unsafe { core::arch::asm!("mv {0}, tp", out(reg) id) }
        id
    }
    #[cfg(test)]
    {
        0
    }
}

/// # Safety
///
/// Caller contract: must run once during early kernel init (single-hart boot
/// path) before any hart touches these globals; no concurrent access.
pub unsafe fn init() {
    // SAFETY: one-time boot-time reset of the process globals; by the caller
    // contract no other hart reads or writes them yet.
    unsafe {
        G_ALL_PROCS = ptr::null_mut();
        G_CURRENT = ptr::null_mut();
        for i in 0..MAX_HARTS {
            G_HART_CURRENT[i] = ptr::null_mut();
            G_NEED_RESCHED[i].store(false, Ordering::Release);
        }
        G_NEXT_PID.store(PROC_PID_INIT + 1, Ordering::Release);
    }
}

pub fn alloc_pid() -> u32 {
    // Bug (syscall SERIOUS #6): use atomic fetch_add to avoid races between
    // concurrent fork()/spawn() calls on different harts. The previous
    // non-atomic read-then-write could hand out the same PID to two
    // processes if they raced.
    G_NEXT_PID.fetch_add(1, Ordering::SeqCst)
}

/// # Safety
///
/// Caller contract: out-of-range `hartid` yields null; the slot for `hartid`
/// is written only by the scheduler running on that hart, so a caller reading
/// another hart's slot must tolerate it changing concurrently.
pub unsafe fn current_for_hart(hartid: usize) -> *mut Proc {
    // SAFETY: bounds-checked read of the per-hart slot per the doc contract.
    unsafe {
        if hartid < MAX_HARTS {
            // SAFETY: slot is either null or points to a live heap-allocated
            // Proc (never freed while current).
            G_HART_CURRENT[hartid]
        } else {
            ptr::null_mut()
        }
    }
}

/// # Safety
///
/// Caller contract: `p` must be a valid Proc pointer (or null); the slot for
/// `hartid` is written only by the scheduler running on that hart, and slot 0
/// additionally mirrors into `G_CURRENT` for legacy single-hart paths.
pub unsafe fn set_current_for_hart(hartid: usize, p: *mut Proc) {
    // SAFETY: bounds-checked write by the owning hart's scheduler/boot init.
    unsafe {
        if hartid < MAX_HARTS {
            // SAFETY: slot write per the caller contract; no other hart
            // writes this slot.
            G_HART_CURRENT[hartid] = p;
            if hartid == 0 {
                G_CURRENT = p;
            }
        }
    }
}

/// # Safety
///
/// Caller contract: `hart` must be a valid hart index accepted by
/// `smp::set_cpu_online` (see that module for its bounds contract).
pub unsafe fn set_cpu_online(hart: usize, v: bool) {
    // SAFETY: forward to the smp module; validity of `hart` is the caller
    // contract documented above.
    unsafe {
        smp::set_cpu_online(hart, v);
    }
}

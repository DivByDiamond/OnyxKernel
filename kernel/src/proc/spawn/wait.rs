use onyx_core::errno::{Errno, KResult};

use super::super::process::{
    G_ALL_PROCS, ProcState, current_for_hart, current_pid, hart_id, proc_list_lock,
    proc_list_unlock,
};
use crate::{arch::trap_frame::TrapFrame, mm::heap};

/// # Safety
///
/// Caller contract: process context of the waiting parent on this hart;
/// `status_out` (if non-null) must be a writable i32; must NOT already hold
/// proc_list_lock; may return while parked via sched_yield.
pub unsafe fn wait(tf: &mut TrapFrame, status_out: *mut i32) -> KResult<u32> {
    // SAFETY: the child scan/unlink runs under proc_list_lock (taken here);
    // the reaped node is kfree'd only after unlinking, and the Waiting
    // publish uses the same lock per the B4 lost-wakeup protocol below.
    unsafe {
        let my_pid = current_pid();
        loop {
            proc_list_lock();
            let mut cur = G_ALL_PROCS;
            while !cur.is_null() {
                if (*cur).parent_pid == my_pid && matches!((*cur).state, ProcState::Exited) {
                    let exited_pid = (*cur).pid;
                    let code = (*cur).exit_code;
                    if G_ALL_PROCS == cur {
                        G_ALL_PROCS = (*cur).all_next;
                    } else {
                        let mut walk = G_ALL_PROCS;
                        while !walk.is_null() && (*walk).all_next != cur {
                            walk = (*walk).all_next;
                        }
                        if !walk.is_null() {
                            (*walk).all_next = (*cur).all_next;
                        }
                    }
                    proc_list_unlock();
                    if !status_out.is_null() {
                        *status_out = code;
                    }
                    heap::kfree(cur as *mut u8);
                    return Ok(exited_pid);
                }
                cur = (*cur).all_next;
            }
            let mut has_child = false;
            cur = G_ALL_PROCS;
            while !cur.is_null() {
                if (*cur).parent_pid == my_pid && !matches!((*cur).state, ProcState::Free) {
                    has_child = true;
                    break;
                }
                cur = (*cur).all_next;
            }
            // B4 fix (lost wakeup): publish Waiting under the SAME proc_list_lock
            // critical section that verified has_child. Previously the lock was
            // dropped first and a child could exit in the window before we set
            // Waiting — its wake check would see Running, not Waiting — leaving this
            // parent asleep forever. exit() performs its Exited-publish + parent-wake
            // under this same lock, so either we park first and get woken, or we
            // observe the exited child on retry.
            if !has_child {
                proc_list_unlock();
                return Err(Errno::NoEnt);
            }
            let hartid = hart_id();
            let cur = current_for_hart(hartid);
            if !cur.is_null() {
                (*cur).state = ProcState::Waiting;
            }
            proc_list_unlock();
            // sched_yield only parks us; it does not tell us the child has
            // actually exited yet. Loop back and rescan instead of failing
            // the syscall the first time we're rescheduled — otherwise every
            // `wait()` racing an unfinished child spuriously returns ENOENT
            // (surfacing to userspace as "wait: Invalid argument" through the
            // POSIX errno translation) even though the child is still alive.
            super::super::scheduler::sched_yield(tf);
        }
    }
}

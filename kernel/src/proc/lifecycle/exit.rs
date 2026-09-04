use core::ptr;

use super::{
    G_ALL_PROCS, MAX_HARTS, ProcState, by_pid, by_pid_unlocked, dec_root_refcount, hart_id,
    proc_list_lock, proc_list_unlock,
};
use crate::{
    mm::{heap, vmm},
    proc::scheduler::{rq_lock, rq_unlock},
};

/// # Safety
///
/// Caller contract: called from the exiting process's own context (trap
/// path / signal default action); `pid` identifies a live process.
pub unsafe fn exit(pid: u32, code: i32) {
    // SAFETY: runqueue removals hold each queue's rq_lock; the Exited
    // publish + parent wake + orphan re-parent all run under proc_list_lock
    // (B4 fix, comment below); by_pid takes proc_list_lock itself and the
    // returned node stays valid until reaped.
    unsafe {
        if let Some(p) = by_pid(pid) {
            // Normal exit (code == 0) is INFO, not ERR: logging every clean
            // shutdown as "[ERR] proc: pid N exited code=0" (e.g. after
            // `exec passwd` ends a login session) made users believe the
            // process had crashed (bug report 2026-09-04).
            if code == 0 {
                crate::kinf!(
                    "proc",
                    "pid %d exited code=%d",
                    onyx_core::fmt::Arg::from(pid),
                    onyx_core::fmt::Arg::from(code)
                );
            } else {
                crate::kerr!(
                    "proc",
                    "pid %d exited code=%d",
                    onyx_core::fmt::Arg::from(pid),
                    onyx_core::fmt::Arg::from(code)
                );
            }
            let p_ptr = p as *mut _;
            for h in 0..MAX_HARTS {
                crate::proc::scheduler::rq_lock(h);
                let _ = crate::proc::scheduler::runqueue::remove(h, p_ptr);
                crate::proc::scheduler::rq_unlock(h);
            }
            for i in 0..p.fds.len() {
                if p.fds[i].used {
                    let token = crate::fs::vfs::fd_token(i, p.fds[i].epoch);
                    let _ = crate::fs::vfs::close(token);
                }
            }
            if p.root_pa != 0 {
                // Audit fix: atomic decrement of the SHARED refcount (lost
                // decrement under SMP → premature destroy/UAF or leak).
                if !p.root_refcount.is_null() {
                    if dec_root_refcount(p.root_refcount) {
                        heap::kfree(p.root_refcount as *mut u8);
                        vmm::destroy_root(p.root_pa);
                    }
                } else {
                    vmm::destroy_root(p.root_pa);
                }
                p.root_pa = 0;
                p.root_refcount = ptr::null_mut();
            }
            // B4 fix (lost wakeup): the exit_code write, state=Exited publish,
            // parent wake and orphan re-parenting must ALL happen inside ONE
            // proc_list_lock critical section. Previously exit_code+state were
            // written unlocked and the parent's Waiting check raced with
            // wait()'s "set Waiting AFTER dropping proc_list_lock" window — a
            // child exiting in that window left the parent asleep forever.
            // The parent observes (exit_code, Exited) atomically through the
            // same lock; Release on unlock makes both writes visible before any
            // wake. Lock order matches globals.rs: PROC_LIST_LOCK outermost,
            // rq_lock nested inside.
            p.exit_code = code;
            proc_list_lock();
            p.state = ProcState::Exited;
            let parent = p.parent_pid;
            // Auto-reap flag: a parent with SA_NOCLDWAIT never sees zombies
            // or SIGCHLD (todo P2 #2). The node is unlinked + freed after
            // the lock is dropped (kfree takes proc_list_lock itself).
            let mut auto_reap = false;
            if parent != 0
                && let Some(pp) = by_pid_unlocked(parent)
            {
                if !pp.no_cldwait {
                    pp.pending_signals |= 1u32 << crate::proc::SIGCHLD;
                } else {
                    auto_reap = true;
                }
                if matches!(pp.state, ProcState::Waiting) {
                    pp.state = ProcState::Ready;
                    let caller_hart = hart_id();
                    rq_lock(caller_hart);
                    crate::proc::scheduler::enqueue(caller_hart, pp as *mut _);
                    rq_unlock(caller_hart);
                }
            }
            let mut cur = G_ALL_PROCS;
            while !cur.is_null() {
                if (*cur).parent_pid == pid
                    && !matches!((*cur).state, ProcState::Free | ProcState::Exited)
                {
                    (*cur).parent_pid = 1;
                }
                cur = (*cur).all_next;
            }
            // Auto-reap unlink (SA_NOCLDWAIT): drop the node from the all-
            // procs list inside the same critical section that published
            // Exited, so no other hart can traverse to it afterwards.
            if auto_reap {
                if G_ALL_PROCS == p_ptr {
                    G_ALL_PROCS = (*p_ptr).all_next;
                } else {
                    let mut walk = G_ALL_PROCS;
                    while !walk.is_null() && (*walk).all_next != p_ptr {
                        walk = (*walk).all_next;
                    }
                    if !walk.is_null() {
                        (*walk).all_next = (*p_ptr).all_next;
                    }
                }
            }
            proc_list_unlock();
            if auto_reap {
                // SAFETY: the node was unlinked from G_ALL_PROCS under
                // proc_list_lock above; its runqueue links were removed and
                // root_pa torn down earlier in this fn; no hart can reach it.
                heap::kfree(p_ptr as *mut u8);
            }
        }
    }
}

use super::{by_pid, hart_id, proc_list_lock, proc_list_unlock, ProcState, G_ALL_PROCS, MAX_HARTS};
use crate::mm::{heap, vmm};
use crate::proc::scheduler::{rq_lock, rq_unlock};
use core::ptr;

pub unsafe fn exit(pid: u32, code: i32) {
    if let Some(p) = by_pid(pid) {
        crate::kerr!(
            "proc",
            "pid %d exited code=%d",
            onyx_core::fmt::Arg::from(pid),
            onyx_core::fmt::Arg::from(code)
        );
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
            if !p.root_refcount.is_null() {
                *p.root_refcount -= 1;
                if *p.root_refcount == 0 {
                    heap::kfree(p.root_refcount as *mut u8);
                    vmm::destroy_root(p.root_pa);
                }
            } else {
                vmm::destroy_root(p.root_pa);
            }
            p.root_pa = 0;
            p.root_refcount = ptr::null_mut();
        }
        p.exit_code = code;
        p.state = ProcState::Exited;
        let parent = p.parent_pid;
        if parent != 0 {
            if let Some(pp) = by_pid(parent) {
                if matches!(pp.state, ProcState::Waiting) {
                    pp.state = ProcState::Ready;
                    let caller_hart = hart_id();
                    rq_lock(caller_hart);
                    crate::proc::scheduler::enqueue(caller_hart, pp as *mut _);
                    rq_unlock(caller_hart);
                }
            }
        }
        proc_list_lock();
        let mut cur = G_ALL_PROCS;
        while !cur.is_null() {
            if (*cur).parent_pid == pid
                && !matches!((*cur).state, ProcState::Free | ProcState::Exited)
            {
                (*cur).parent_pid = 1;
            }
            cur = (*cur).all_next;
        }
        proc_list_unlock();
    }
}

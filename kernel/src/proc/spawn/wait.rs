use super::super::process::{
    current_for_hart, current_pid, hart_id, proc_list_lock, proc_list_unlock, ProcState,
    G_ALL_PROCS,
};
use crate::arch::trap_frame::TrapFrame;
use crate::mm::heap;
use onyx_core::errno::{Errno, KResult};

pub unsafe fn wait(tf: &mut TrapFrame, status_out: *mut i32) -> KResult<u32> {
    let my_pid = current_pid();
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
    proc_list_unlock();
    if !has_child {
        return Err(Errno::NoEnt);
    }
    let hartid = hart_id();
    let cur = current_for_hart(hartid);
    if !cur.is_null() {
        (*cur).state = ProcState::Waiting;
    }
    super::super::scheduler::sched_yield(tf);
    Err(Errno::NoEnt)
}

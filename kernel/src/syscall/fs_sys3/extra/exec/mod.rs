mod fork;

pub(super) use fork::*;

use onyx_core::errno::Errno;

use crate::arch::trap_frame::TrapFrame;
use crate::mm::vmm;
use crate::proc;
use crate::proc::process::{proc_list_lock, proc_list_unlock, ProcState};
use crate::syscall::abi::WNOHANG;
use crate::syscall::handler::user_ptr_ok;

pub unsafe fn sys_waitpid(tf: &mut TrapFrame, pid: u64, status_out: u64, options: u32) -> i64 {
    let my_pid = proc::current_pid();

    if status_out != 0 && !user_ptr_ok(status_out, 4) {
        return Errno::Inval.as_i64();
    }

    proc_list_lock();

    let mut cur = proc::G_ALL_PROCS;
    while !cur.is_null() {
        if (*cur).parent_pid == my_pid && matches!((*cur).state, ProcState::Exited) {
            let matches_pid = if pid == u32::MAX as u64 || pid == 0 {
                true
            } else if (pid as i64) < 0 {
                true
            } else {
                (*cur).pid == pid as u32
            };
            if matches_pid {
                let exited_pid = (*cur).pid;
                let code = (*cur).exit_code;
                if proc::G_ALL_PROCS == cur {
                    proc::G_ALL_PROCS = (*cur).all_next;
                } else {
                    let mut walk = proc::G_ALL_PROCS;
                    while !walk.is_null() && (*walk).all_next != cur {
                        walk = (*walk).all_next;
                    }
                    if !walk.is_null() {
                        (*walk).all_next = (*cur).all_next;
                    }
                }
                proc_list_unlock();
                if status_out != 0 {
                    let pa = crate::mm::vmm::translate(proc::current().root_pa, status_out);
                    if pa != 0 {
                        *(pa as *mut i32) = code;
                    }
                }
                crate::mm::heap::kfree(cur as *mut u8);
                return exited_pid as i64;
            }
        }
        cur = (*cur).all_next;
    }

    let mut has_child = false;
    cur = proc::G_ALL_PROCS;
    while !cur.is_null() {
        if (*cur).parent_pid == my_pid && !matches!((*cur).state, ProcState::Free) {
            let matches_pid = if pid == u32::MAX as u64 || pid == 0 {
                true
            } else if (pid as i64) < 0 {
                true
            } else {
                (*cur).pid == pid as u32
            };
            if matches_pid {
                has_child = true;
                break;
            }
        }
        cur = (*cur).all_next;
    }
    proc_list_unlock();
    if !has_child {
        return Errno::Child.as_i64();
    }

    if options & WNOHANG != 0 {
        return 0;
    }

    let hartid = proc::hart_id();
    let cur = proc::current_for_hart(hartid);
    if !cur.is_null() {
        (*cur).state = ProcState::Waiting;
    }
    crate::proc::scheduler::sched_yield(tf);
    Errno::NoEnt.as_i64()
}

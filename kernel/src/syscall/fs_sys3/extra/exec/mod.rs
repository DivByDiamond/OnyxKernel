mod fork;

pub use fork::*;

use onyx_core::errno::Errno;

use crate::arch::trap_frame::TrapFrame;
use crate::proc;
use crate::proc::process::{ProcState, proc_list_lock, proc_list_unlock};
use crate::syscall::abi::WNOHANG;
use crate::syscall::handler::user_ptr_ok;

pub unsafe fn sys_waitpid(tf: &mut TrapFrame, pid: u64, status_out: u64, options: u32) -> i64 {
    unsafe {
        let my_pid = proc::current_pid();

        if status_out != 0
            && (!user_ptr_ok(status_out, 4)
                || crate::mm::vmm::check_user_range(proc::current().root_pa, status_out, 4, true)
                    .is_err())
        {
            return Errno::Fault.as_i64();
        }

        proc_list_lock();

        let mut cur = proc::G_ALL_PROCS;
        while !cur.is_null() {
            if (*cur).parent_pid == my_pid && matches!((*cur).state, ProcState::Exited) {
                // Any special pid selector (all / any / negative) matches every child.
                let matches_pid = pid == u32::MAX as u64
                    || pid == 0
                    || (pid as i64) < 0
                    || (*cur).pid == pid as u32;
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
                        let code_buf = code.to_ne_bytes();
                        if crate::mm::vmm::copy_to_user(
                            proc::current().root_pa,
                            status_out,
                            code_buf.as_ptr(),
                            4,
                        )
                        .is_err()
                        {
                            return Errno::Fault.as_i64();
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
                // Any special pid selector (all / any / negative) matches every child.
                let matches_pid = pid == u32::MAX as u64
                    || pid == 0
                    || (pid as i64) < 0
                    || (*cur).pid == pid as u32;
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
}

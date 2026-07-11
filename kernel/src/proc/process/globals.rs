use crate::arch::trap_frame::TrapFrame;
use core::ptr;
use core::sync::atomic::{AtomicBool, Ordering};

use super::types::{PROC_PID_INIT, Proc};
use crate::arch::smp;

pub const MAX_HARTS: usize = smp::MAX_HARTS;

pub static mut G_ALL_PROCS: *mut Proc = ptr::null_mut();

pub static mut G_HART_CURRENT: [*mut Proc; MAX_HARTS] = [ptr::null_mut(); MAX_HARTS];

pub static mut G_HART_IDLE_TF: [TrapFrame; MAX_HARTS] = [TrapFrame::zero(); MAX_HARTS];

pub static G_NEED_RESCHED: [AtomicBool; MAX_HARTS] = [const { AtomicBool::new(false) }; MAX_HARTS];

pub static mut G_CURRENT: *mut Proc = ptr::null_mut();

pub static mut G_NEXT_PID: u32 = PROC_PID_INIT;

#[inline]
pub fn hart_id() -> usize {
    let id: usize;
    unsafe { core::arch::asm!("mv {0}, tp", out(reg) id) }
    id
}

pub unsafe fn init() {
    G_ALL_PROCS = ptr::null_mut();
    G_CURRENT = ptr::null_mut();
    for i in 0..MAX_HARTS {
        G_HART_CURRENT[i] = ptr::null_mut();
        G_NEED_RESCHED[i].store(false, Ordering::Release);
    }
    G_NEXT_PID = PROC_PID_INIT;
}

pub fn alloc_pid() -> u32 {
    unsafe {
        let pid = G_NEXT_PID;
        G_NEXT_PID = pid + 1;
        pid
    }
}

pub unsafe fn current_for_hart(hartid: usize) -> *mut Proc {
    if hartid < MAX_HARTS {
        G_HART_CURRENT[hartid]
    } else {
        ptr::null_mut()
    }
}

pub unsafe fn set_current_for_hart(hartid: usize, p: *mut Proc) {
    if hartid < MAX_HARTS {
        G_HART_CURRENT[hartid] = p;
        if hartid == 0 {
            G_CURRENT = p;
        }
    }
}

pub unsafe fn set_cpu_online(hart: usize, v: bool) {
    smp::set_cpu_online(hart, v);
}

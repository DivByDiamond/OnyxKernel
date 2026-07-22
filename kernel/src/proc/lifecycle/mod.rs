use super::process::Proc;
use super::process::{
    by_pid, hart_id, proc_list_lock, proc_list_unlock, set_current_for_hart, ProcState,
    G_ALL_PROCS, PROC_RING_KERNEL,
};
use crate::arch::trap_frame::TrapFrame;
use crate::mm::{heap, vmm};
use core::ptr;
use onyx_core::errno::KResult;

mod exit;

pub use exit::*;

pub(super) unsafe fn alloc_proc() -> KResult<*mut Proc> {
    let p = heap::kmalloc(core::mem::size_of::<Proc>())? as *mut Proc;
    ptr::write_bytes(p as *mut u8, 0, core::mem::size_of::<Proc>());
    (*p).pid = 0;
    (*p).ring = PROC_RING_KERNEL;
    (*p).state = ProcState::Free;
    (*p).parent_pid = 0;
    (*p).exit_code = 0;
    (*p).root_pa = 0;
    (*p).entry = 0;
    (*p).ustack = 0;
    (*p).heap_brk = 0;
    (*p).mmap_brk = 0x3000_0000;
    (*p).uid = 0;
    (*p).gid = 0;
    (*p).cwd[0] = b'/';
    (*p).cwd[1] = 0;
    (*p).cwd_len = 1;
    (*p).tf = TrapFrame::zero();
    (*p).pending_signals = 0;
    (*p).signal_mask = 0;
    for h in (*p).signal_handlers.iter_mut() {
        *h = 0;
    }
    for m in (*p).signal_handler_masks.iter_mut() {
        *m = 0;
    }
    (*p).saved_mask = 0;
    (*p).saved_tf = TrapFrame::zero();
    (*p).in_signal_handler = false;
    for fd in (*p).fds.iter_mut() {
        *fd = crate::fs::vfs::VfsFd::default();
    }
    (*p).wait_next = ptr::null_mut();
    (*p).all_next = G_ALL_PROCS;
    (*p).affinity = -1;
    (*p).on_rq = false;
    (*p).raw_stdin = false;
    G_ALL_PROCS = p;
    Ok(p)
}

pub unsafe fn free_proc(p: *mut Proc) {
    proc_list_lock();
    if G_ALL_PROCS == p {
        G_ALL_PROCS = (*p).all_next;
    } else {
        let mut cur = G_ALL_PROCS;
        while !cur.is_null() && (*cur).all_next != p {
            cur = (*cur).all_next;
        }
        if !cur.is_null() {
            (*cur).all_next = (*p).all_next;
        }
    }
    proc_list_unlock();
    heap::kfree(p as *mut u8);
}

pub unsafe fn enter_user(pid: u32) -> ! {
    let mut p = G_ALL_PROCS;
    while !p.is_null() {
        if (*p).pid == pid && !matches!((*p).state, ProcState::Free) {
            break;
        }
        p = (*p).all_next;
    }
    if p.is_null() {
        crate::srv::klog::puts("proc: enter_user: pid not found, halting\n");
        crate::srv::klog::halt();
    }
    (*p).state = ProcState::Running;
    let hartid = super::process::hart_id();
    set_current_for_hart(hartid, p);
    let entry = (*p).entry as usize;
    let ustack = (*p).ustack as usize;
    let root_pa = (*p).root_pa as usize;
    crate::arch::asm::drop_to_user(entry, ustack, root_pa)
}

pub fn count() -> usize {
    unsafe {
        let mut n = 0;
        let mut cur = G_ALL_PROCS;
        while !cur.is_null() {
            if !matches!((*cur).state, ProcState::Free) {
                n += 1;
            }
            cur = (*cur).all_next;
        }
        n
    }
}

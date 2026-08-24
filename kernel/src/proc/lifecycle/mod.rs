use core::{
    ptr,
    sync::atomic::{AtomicU32, Ordering},
};

use onyx_core::errno::KResult;

use super::process::{
    G_ALL_PROCS, MAX_HARTS, PROC_RING_KERNEL, Proc, ProcState, by_pid, by_pid_unlocked, hart_id,
    proc_list_lock, proc_list_unlock, set_current_for_hart,
};
use crate::{arch::trap_frame::TrapFrame, mm::heap};

mod exit;

pub use exit::*;

/// Atomically drop one reference on a shared root-page-table refcount.
///
/// Audit fix: `root_refcount` is a `*mut u32` SHARED between fork parent and
/// children, so the previous plain `*rc -= 1` could lose a decrement when two
/// harts exited concurrently (→ premature destroy/UAF or leak). The heap
/// allocation stays a 4-byte `u32` (layout unchanged); all accessors must go
/// through this helper using atomic RMW.
///
/// Returns `true` when the caller dropped the LAST reference and must free
/// the refcount cell and destroy the page table.
pub unsafe fn dec_root_refcount(rc: *mut u32) -> bool {
    unsafe {
        if rc.is_null() {
            return false;
        }
        AtomicU32::from_ptr(rc).fetch_sub(1, Ordering::AcqRel) == 1
    }
}

pub(super) unsafe fn alloc_proc() -> KResult<*mut Proc> {
    unsafe {
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
        // Plant the kstack overflow canary (see KSTACK_CANARY in types.rs).
        ptr::write_volatile(
            (*p).kstack.as_mut_ptr() as *mut u64,
            crate::proc::KSTACK_CANARY,
        );
        G_ALL_PROCS = p;
        Ok(p)
    }
}

pub unsafe fn free_proc(p: *mut Proc) {
    unsafe {
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
}

pub unsafe fn enter_user(pid: u32) -> ! {
    unsafe {
        crate::srv::klog::debug_mark(b'U');
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

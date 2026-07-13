//! Process lifecycle — allocation, freeing, `enter_user`, `exit`, and `count`.
use super::process::Proc;
use super::process::{
    by_pid, hart_id, proc_list_lock, proc_list_unlock, set_current_for_hart, ProcState, G_ALL_PROCS,
    PROC_RING_KERNEL,
};
use crate::arch::trap_frame::TrapFrame;
use crate::mm::{heap, vmm};
use crate::proc::scheduler::{rq_lock, rq_unlock};
use core::ptr;
use onyx_core::errno::KResult;

/// Allocate a new Proc node on the heap and add it to the list.
pub(super) unsafe fn alloc_proc() -> KResult<*mut Proc> {
    let p = heap::kmalloc(core::mem::size_of::<Proc>())? as *mut Proc;
    // Zero the entire struct.
    ptr::write_bytes(p as *mut u8, 0, core::mem::size_of::<Proc>());
    // Initialize fields (kmalloc may not zero — depends on SLAB vs free-list).
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
    (*p).tf = TrapFrame::zero();
    (*p).pending_signals = 0;
    (*p).signal_mask = 0;
    // Initialize per-process signal handler table — all defaults (0).
    for h in (*p).signal_handlers.iter_mut() {
        *h = 0;
    }
    for m in (*p).signal_handler_masks.iter_mut() {
        *m = 0;
    }
    (*p).saved_mask = 0;
    (*p).saved_tf = TrapFrame::zero();
    (*p).in_signal_handler = false;
    // Initialize per-process FD table — all slots free.
    for fd in (*p).fds.iter_mut() {
        *fd = crate::fs::vfs::VfsFd::default();
    }
    (*p).wait_next = ptr::null_mut();
    (*p).all_next = G_ALL_PROCS;
    G_ALL_PROCS = p;
    Ok(p)
}

/// Free a Proc node from the list and heap.
///
/// Bug #16 fix: hold proc_list_lock while unlinking from G_ALL_PROCS so
/// two harts reaping the same exited child via wait()/waitpid() can't
/// race past each other and double-kfree the node.
pub unsafe fn free_proc(p: *mut Proc) {
    proc_list_lock();
    // Remove from process list.
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
    // Find process by pid.
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
    // Set per-hart current (hart 0 for the primary bootstrap).
    let hartid = super::process::hart_id();
    set_current_for_hart(hartid, p);
    let entry = (*p).entry as usize;
    let ustack = (*p).ustack as usize;
    let root_pa = (*p).root_pa as usize;
    crate::arch::asm::drop_to_user(entry, ustack, root_pa)
}

pub unsafe fn exit(pid: u32, code: i32) {
    if let Some(p) = by_pid(pid) {
        crate::kerr!(
            "proc",
            "pid %d exited code=%d",
            onyx_core::fmt::Arg::from(pid),
            onyx_core::fmt::Arg::from(code)
        );
        // Bug #15 fix: remove the process from every hart's runqueue BEFORE
        // setting state to Exited. Without this, the process could still be
        // on a runqueue with on_rq=true, and the next sched_yield on that
        // hart would dequeue it, see state != Running (it's Exited), skip
        // the re-enqueue, but then promote it back to Running at the bottom
        // of sched_yield — effectively reviving a dead process. Iterating
        // all harts is O(MAX_HARTS) but MAX_HARTS is small, and remove()
        // returns immediately when on_rq is false.
        let p_ptr = p as *mut Proc;
        for h in 0..super::process::MAX_HARTS {
            crate::proc::scheduler::rq_lock(h);
            let _ = crate::proc::scheduler::runqueue::remove(h, p_ptr);
            crate::proc::scheduler::rq_unlock(h);
        }
        // Close all open file descriptors so kernel-internal file resources
        // (OnyxFS inodes, pipe buffers, etc.) are released. The FD slots
        // themselves live in the Proc struct and will be freed with it.
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
        // If parent is waiting, wake it up.
        let parent = p.parent_pid;
        if parent != 0 {
            if let Some(pp) = by_pid(parent) {
                if matches!(pp.state, ProcState::Waiting) {
                    pp.state = ProcState::Ready;
                    // Bug #11 fix: acquire rq_lock before enqueue. The
                    // previous code called enqueue(hart_id(), pp) with no
                    // lock, racing with the scheduler on this hart.
                    let caller_hart = hart_id();
                    rq_lock(caller_hart);
                    crate::proc::scheduler::enqueue(caller_hart, pp as *mut Proc);
                    rq_unlock(caller_hart);
                }
            }
        }
        // Re-parent any orphaned children to PID 1 (init). This prevents
        // zombie leaks when a parent dies before its children. The init
        // process is expected to call `wait()` periodically to reap them.
        //
        // Bug #16 fix: hold proc_list_lock while walking G_ALL_PROCS —
        // another hart may concurrently be freeing a process and
        // unlinking it from the list, which would orphan our cursor.
        proc_list_lock();
        let mut cur = G_ALL_PROCS;
        while !cur.is_null() {
            if (*cur).parent_pid == pid
                && !matches!((*cur).state, ProcState::Free | ProcState::Exited)
            {
                (*cur).parent_pid = 1; // init reaps orphans
            }
            cur = (*cur).all_next;
        }
        proc_list_unlock();
    }
}

/// Count active processes (for diagnostics).
///
/// Bug (syscall SERIOUS #8): the previous code walked `(*cur).next`
/// (the runqueue link) instead of `(*cur).all_next` (the global
/// process-list link). For processes not on any runqueue, `next` is
/// null, so count() would stop at the first non-runqueue process and
/// massively undercount. Now walks all_next.
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

//! Process spawning and waiting — `create_user`, `spawn`, `wait`.
//!
//! `spawn` is the SYS_spawn implementation: it loads a .onx file via the VFS,
//! constructs a fresh process via `create_user`, and returns its PID. `wait`
//! is SYS_wait: it blocks the caller until a child exits, then reaps it.
use super::lifecycle::{alloc_proc, free_proc};
use super::process::{
    alloc_pid, by_pid, current_for_hart, current_pid, hart_id, proc_list_lock, proc_list_unlock,
    ProcState, G_ALL_PROCS, PROC_RING_ROOT, PROC_RING_USER,
};
use crate::arch::regs::*;
use crate::arch::trap_frame::TrapFrame;
use crate::mm::heap;
use crate::proc::onx;
use crate::proc::scheduler::{enqueue, rq_lock, rq_unlock};
use onyx_core::errno::{Errno, KResult};

/// Create a user-mode process (ring 1 or 2). Dynamic allocation — no limit.
///
/// `root_refcount` — if non-null, shares the root page table refcount with the
/// caller (used by fork). If null, a new refcount (initialised to 1) is
/// allocated, indicating the process owns its page table.
pub unsafe fn create_user(
    entry: u64,
    ustack: u64,
    root_pa: u64,
    pid: u32,
    parent_pid: u32,
    heap_brk: u64,
    ring: u8,
    argc: usize,
    argv_sp: u64,
    root_refcount: *mut u32,
) -> KResult<()> {
    if entry == 0 {
        crate::kerr!("create_user", "entry=0 — would cause page fault, rejecting");
        return Err(Errno::Inval);
    }
    let p = alloc_proc()?;
    (*p).pid = pid;
    (*p).ring = ring;
    (*p).state = ProcState::Ready;
    (*p).parent_pid = parent_pid;
    (*p).exit_code = 0;
    (*p).root_pa = root_pa;
    if !root_refcount.is_null() {
        (*p).root_refcount = root_refcount;
    } else {
        // Process owns its own page table — allocate a refcount of 1.
        let rc = heap::kmalloc(4)? as *mut u32;
        *rc = 1;
        (*p).root_refcount = rc;
    }
    (*p).entry = entry;
    (*p).ustack = ustack;
    (*p).heap_brk = heap_brk;
    (*p).uid = if ring <= PROC_RING_ROOT { 0 } else { 1000 };
    (*p).gid = (*p).uid;
    (*p).tf = TrapFrame::zero();
    (*p).pending_signals = 0;
    (*p).signal_mask = 0;
    (*p).tf.sepc = entry;
    (*p).tf.sp = if argc > 0 { argv_sp } else { ustack };
    (*p).tf.a0 = argc as u64;
    (*p).tf.a1 = if argc > 0 { argv_sp + 8 } else { 0 };
    (*p).tf.sstatus = SSTATUS_SPIE;
    (*p).tf.satp = SATP_MODE_SV39 | (root_pa >> 12);
    // Bug #11 fix: hold the caller's rq_lock around enqueue. The previous
    // code called enqueue(hart_id(), p) with no lock, racing with the
    // scheduler on the same hart and producing orphaned/duplicated entries.
    let caller_hart = hart_id();
    rq_lock(caller_hart);
    enqueue(caller_hart, p);
    rq_unlock(caller_hart);
    Ok(())
}

/// **SYS_spawn**: create a new process from .onx file without replacing current.
pub unsafe fn spawn(path: &[u8], argv_user: u64, ring_hint: u8, parent_pid: u32) -> KResult<u32> {
    use crate::fs::vfs;
    let token = vfs::open(path, vfs::PERM_READ | vfs::PERM_SEEK)?;
    let mut size = 0u32;
    vfs::stat(token, &mut size)?;
    if size == 0 {
        vfs::close(token)?;
        return Err(Errno::Inval);
    }
    let img = heap::kmalloc(size as usize)?;
    vfs::read(token, img, size)?;
    vfs::close(token)?;
    let r = onx::load(img, size as usize)?;
    heap::kfree(img);
    let new_pid = alloc_pid();
    let ring = if ring_hint == PROC_RING_ROOT && r.ring == 1 {
        PROC_RING_ROOT
    } else {
        PROC_RING_USER
    };
    let (argc, argv_sp) = if argv_user != 0 {
        crate::proc::onx::copy_argv_to_stack(r.root_pa, r.ustack, argv_user)
    } else {
        (0, 0)
    };
    create_user(
        r.entry,
        r.ustack,
        r.root_pa,
        new_pid,
        parent_pid,
        r.heap_brk,
        ring,
        argc,
        argv_sp,
        core::ptr::null_mut(),
    )?;
    Ok(new_pid)
}

/// **SYS_wait**: wait for any child to exit. Returns (pid, exit_code).
/// Blocks (sets current state to `Waiting` and yields) if no child has exited
/// yet but at least one child exists. The process is woken when a child calls
/// `exit()` (which transitions the parent back to `Ready`). Returns ENOENT if
/// the caller has no children at all.
///
/// Note on the control-flow quirk: `sched_yield` does not return to its caller
/// when it actually switches — it `sret`s to user space using the saved trap
/// frame. Because `tf.sepc` was not yet advanced past the `ecall` instruction
/// (we are still inside `handle()`), the user process re-executes the `ecall`,
/// re-entering `wait()` from the top. The loop below therefore executes at
/// most one iteration per `ecall`; the "retry" happens via re-ecall.
pub unsafe fn wait(tf: &mut TrapFrame, status_out: *mut i32) -> KResult<u32> {
    let my_pid = current_pid();
    // Bug #16 fix: hold proc_list_lock around the iteration + free_proc so
    // two harts can't simultaneously reap the same exited child and
    // double-kfree the Proc node. The lock is released before sched_yield
    // (which may rq_lock and switch away) to preserve lock ordering.
    proc_list_lock();
    // Look for exited child.
    let mut cur = G_ALL_PROCS;
    while !cur.is_null() {
        if (*cur).parent_pid == my_pid && matches!((*cur).state, ProcState::Exited) {
            let exited_pid = (*cur).pid;
            let code = (*cur).exit_code;
            // Detach from the list first (still under lock), then drop the
            // lock before doing the actual kfree so we don't hold the
            // global lock during heap operations.
            //
            // We can't call free_proc here because it also takes the
            // lock — instead we inline the unlink and call kfree after
            // releasing the lock.
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
    // Check if any child exists.
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
    // Block: set state to Waiting and yield. `sched_yield` either switches to
    // another Ready process (and `sret`s away — control does not return here)
    // or, if no other process can run, halts the kernel (deadlock detected).
    // The `Err` below is unreachable in practice but keeps the type system
    // happy.
    let hartid = hart_id();
    let cur = current_for_hart(hartid);
    if !cur.is_null() {
        (*cur).state = ProcState::Waiting;
    }
    super::scheduler::sched_yield(tf);
    Err(Errno::NoEnt)
}

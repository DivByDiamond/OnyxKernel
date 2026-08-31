use super::lifecycle::{alloc_proc, free_proc};
use super::process::{
    PROC_PID_INIT, PROC_RING_ROOT, PROC_RING_USER, Proc, ProcState, alloc_pid, by_pid_unlocked,
    hart_id, proc_list_lock, proc_list_unlock,
};
use crate::arch::regs::*;
use crate::arch::trap_frame::{TrapFrame, reg_truncate};
use crate::mm::heap;
use crate::proc::onx;
use crate::proc::scheduler::{enqueue, rq_lock, rq_unlock};
use onyx_core::errno::{Errno, KResult};

mod wait;

pub use wait::*;

/// # Safety
///
/// Caller contract: `root_pa` is a freshly-created user root table owned by
/// the new process; `root_refcount` (if non-null) points at a shared 4-byte
/// refcount cell; on success the caller must not destroy root_pa (the proc
/// owns it); on failure create_user keeps ownership semantics unchanged.
///
/// Fork-race fix (todo P1 #1): on success the new node is left in
/// [`ProcState::Creating`] and is NOT placed on any runqueue. The caller
/// must finish copying all inherited state (fds, signal handlers, cwd,
/// trap frame, ...) and then atomically publish the child with
/// [`publish_ready`]. Until that point no work-stealing hart can observe a
/// half-initialized child: the scheduler only ever dequeues nodes that
/// were explicitly enqueued by `publish_ready`.
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
    // SAFETY: alloc_proc's node is fully initialized before publication; the
    // node is linked into G_ALL_PROCS (state Creating, never enqueued) so
    // waitpid/by_pid can find it; root_refcount (when shared) is only
    // accessed via atomic dec_root_refcount.
    unsafe {
        if entry == 0 {
            crate::kerr!("create_user", "entry=0 — would cause page fault, rejecting");
            return Err(Errno::Inval);
        }
        // Fork-bomb guard: global + per-uid process caps (EAGAIN, like POSIX
        // fork(2) under RLIMIT_NPROC). Checked before any allocation so the
        // failure path has no half-built resources.
        let uid = super::limits::uid_for_ring(ring);
        super::limits::can_create_proc(uid)?;
        let p = alloc_proc()?;
        (*p).pid = pid;
        (*p).ring = ring;
        // Creating, not Ready: the node is visible in G_ALL_PROCS (waitpid
        // has_child scans treat it as a live child) but deliberately not
        // runnable until publish_ready runs.
        (*p).state = ProcState::Creating;
        (*p).parent_pid = parent_pid;
        (*p).exit_code = 0;
        (*p).root_pa = root_pa;
        if !root_refcount.is_null() {
            (*p).root_refcount = root_refcount;
        } else {
            // On allocation failure the half-built Proc node must be released:
            // it is already linked into G_ALL_PROCS by alloc_proc.
            let rc = match heap::kmalloc(4) {
                Ok(rc) => rc,
                Err(e) => {
                    free_proc(p);
                    return Err(e);
                }
            };
            let rc = rc as *mut u32;
            *rc = 1;
            (*p).root_refcount = rc;
        }
        (*p).entry = entry;
        (*p).ustack = ustack;
        (*p).heap_brk = heap_brk;
        (*p).uid = uid;
        (*p).gid = uid;
        (*p).tf = TrapFrame::zero();
        (*p).pending_signals = 0;
        (*p).signal_mask = 0;
        (*p).tf.sepc = reg_truncate(entry);
        (*p).tf.sp = reg_truncate(if argc > 0 { argv_sp } else { ustack });
        (*p).tf.a0 = reg_truncate(argc as u64);
        (*p).tf.a1 = reg_truncate(if argc > 0 { argv_sp + 8 } else { 0 });
        (*p).tf.sstatus = reg_truncate(SSTATUS_SPIE | crate::arch::regs::SSTATUS_FS_INITIAL);
        #[cfg(target_pointer_width = "64")]
        {
            (*p).tf.satp = SATP_MODE_SV39 | (root_pa >> 12);
        }
        #[cfg(target_pointer_width = "32")]
        {
            (*p).tf.satp = crate::arch::bits::SATP_MODE_SV32
                | (((root_pa >> 12) & 0x3FF_FFFF) as crate::arch::trap_frame::Reg);
        }
        // Every newly created process becomes the foreground process (console
        // Ctrl+C target) until the next one is created. init (pid 1) never
        // becomes foreground — Ctrl+C must not kill pid 1.
        if pid != PROC_PID_INIT {
            super::signals::set_foreground(pid);
        }
        Ok(())
    }
}

/// Publish a `Creating` process as runnable: atomically transition
/// Creating → Ready and enqueue it on the CALLING hart's runqueue.
///
/// Fork-race fix (todo P1 #1): this is the single publication point for
/// fork/spawn children. The state write and the enqueue happen under the
/// process-list lock (PROC_LIST_LOCK outermost, rq_lock nested — same lock
/// order as the exit path's B4 fix), so a concurrent `waitpid`/`exit`
/// observer either sees the child still Creating (not yet on any runqueue)
/// or fully Ready+enqueued, never a half-published mix.
///
/// Returns `true` if `pid` identified a Creating process that is now
/// runnable; `false` for unknown pids or processes in any other state
/// (double-publish is rejected — the caller must publish exactly once).
///
/// # Safety
///
/// Caller contract: must NOT already hold proc_list_lock (self-deadlock);
/// `pid` must have been allocated by this hart's own fork/spawn path.
pub unsafe fn publish_ready(pid: u32) -> bool {
    // SAFETY: list traversal and the state/enqueue publication run under
    // proc_list_lock taken here; rq_lock is nested inside (documented
    // order), so no runqueue can observe the node unlocked.
    unsafe {
        proc_list_lock();
        let published = match by_pid_unlocked(pid) {
            Some(p) if matches!(p.state, ProcState::Creating) => {
                p.state = ProcState::Ready;
                let caller_hart = hart_id();
                rq_lock(caller_hart);
                enqueue(caller_hart, p as *mut Proc);
                rq_unlock(caller_hart);
                true
            }
            _ => false,
        };
        proc_list_unlock();
        published
    }
}

/// # Safety
///
/// Caller contract: kernel context with SIE clear; `path` is a valid NUL-
/// terminated kernel slice; `argv_user` (if non-zero) must be a valid user
/// pointer in the PARENT address space validated by the syscall layer.
pub unsafe fn spawn(path: &[u8], argv_user: u64, ring_hint: u8, parent_pid: u32) -> KResult<u32> {
    // SAFETY: vfs token/heap buffer handling is safe within the kernel;
    // onx::load and copy_argv_to_stack operate on the freshly allocated
    // root table and kernel-owned buffers per their own caller contracts.
    unsafe {
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
        let r = match onx::load(img, size as usize) {
            Ok(r) => r,
            Err(e) => {
                heap::kfree(img);
                return Err(e);
            }
        };
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
        if let Err(e) = create_user(
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
        ) {
            // The caller owns the freshly-loaded address space; without this
            // the user pages stay mapped (and accounted) forever.
            crate::mm::vmm::destroy_root(r.root_pa);
            return Err(e);
        }
        // Fork-race fix: spawn copied ALL inherited state inside create_user
        // (fresh image, zeroed fds), so publish immediately. create_user
        // guarantees the child is Creating here; make it runnable.
        if !publish_ready(new_pid) {
            // Unreachable by contract (create_user left the node Creating);
            // keep the address-space accounting honest if it ever trips.
            crate::mm::vmm::destroy_root(r.root_pa);
            return Err(Errno::Inval);
        }
        Ok(new_pid)
    }
}

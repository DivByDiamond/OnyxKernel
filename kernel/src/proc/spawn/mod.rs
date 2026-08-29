use super::lifecycle::{alloc_proc, free_proc};
use super::process::{
    PROC_PID_INIT, PROC_RING_ROOT, PROC_RING_USER, ProcState, alloc_pid, hart_id,
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
    // SAFETY: alloc_proc's node is fully initialized before enqueue; the
    // enqueue happens under rq_lock(caller_hart); root_refcount (when
    // shared) is only accessed via atomic dec_root_refcount.
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
        (*p).state = ProcState::Ready;
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
        let caller_hart = hart_id();
        rq_lock(caller_hart);
        enqueue(caller_hart, p);
        rq_unlock(caller_hart);
        // Every newly created process becomes the foreground process (console
        // Ctrl+C target) until the next one is created. init (pid 1) never
        // becomes foreground — Ctrl+C must not kill pid 1.
        if pid != PROC_PID_INIT {
            super::signals::set_foreground(pid);
        }
        Ok(())
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
        Ok(new_pid)
    }
}

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

mod wait;

pub use wait::*;

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
    if cfg!(target_pointer_width = "64") {
        (*p).tf.satp = SATP_MODE_SV39 | (root_pa >> 12);
    } else {
        (*p).tf.satp = (crate::arch::bits::SATP_MODE_SV32 as u32
            | ((root_pa >> 12) & 0x3FFFFF) as u32)
            as crate::arch::trap_frame::Reg;
    }
    let caller_hart = hart_id();
    rq_lock(caller_hart);
    enqueue(caller_hart, p);
    rq_unlock(caller_hart);
    Ok(())
}

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

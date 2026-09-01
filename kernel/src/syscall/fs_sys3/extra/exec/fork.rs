use onyx_core::errno::Errno;

use crate::arch::trap_frame::{TrapFrame, reg_truncate};
use crate::fs::vfs;
use crate::proc;
use crate::syscall::handler::parse_user_path;

/// # Safety
///
/// Call only from handler::handle's syscall path: current process set, ACL
/// checked; `path`/`argv`/`envp` are validated inside before use.
pub unsafe fn sys_execve(tf: &mut TrapFrame, path: u64, argv: u64, envp: u64) -> i64 {
    // SAFETY: parse_user_path validates the path internally; root_refcount
    // is this process's own live heap u32 (null-checked) and the old root
    // table is only destroyed once its refcount drops to zero.
    unsafe {
        let mut path_buf = [0u8; 256];
        let path_len = match parse_user_path(path, &mut path_buf) {
            Some(l) => l,
            None => return Errno::Inval.as_i64(),
        };
        let path_bytes = &path_buf[..path_len];
        let cur_ring = proc::current_ring();
        let token = match vfs::open(path_bytes, vfs::PERM_READ | vfs::PERM_SEEK) {
            Ok(t) => t,
            Err(e) => return e.as_i64(),
        };
        let mut size = 0u32;
        vfs::stat(token, &mut size).ok();
        let img = match crate::mm::heap::kmalloc(size as usize) {
            Ok(p) => p,
            Err(e) => return e.as_i64(),
        };
        vfs::read(token, img, size).ok();
        vfs::close(token).ok();
        let r = match proc::onx::load(img, size as usize) {
            Ok(r) => r,
            Err(e) => {
                crate::mm::heap::kfree(img);
                return e.as_i64();
            }
        };
        crate::mm::heap::kfree(img);
        if cur_ring == proc::PROC_RING_USER && r.ring == 1 {
            return Errno::Perm.as_i64();
        }
        let uid = proc::current().uid as u64;
        let (argc, argv_sp) =
            proc::onx::copy_argv_envp_to_stack(r.root_pa, r.ustack, argv, envp, r.entry, uid);
        // RISC-V exec ABI stack layout built above:
        //   [argc][argv...][NULL][envp...][NULL][auxv][strings]
        // so envp starts right after argv's NULL terminator.
        let envp_sp = argv_sp + 8 + (argc as u64 + 1) * 8;
        let p = proc::current();

        for i in 0..p.fds.len() {
            if p.fds[i].used && p.fds[i].cloexec {
                let token = crate::fs::vfs::fd_token(i, p.fds[i].epoch);
                let _ = crate::fs::vfs::close(token);
            }
        }

        if p.root_pa != 0 {
            if !p.root_refcount.is_null() {
                *p.root_refcount -= 1;
                if *p.root_refcount == 0 {
                    crate::mm::heap::kfree(p.root_refcount as *mut u8);
                    crate::mm::vmm::destroy_root(p.root_pa);
                }
            } else {
                crate::mm::vmm::destroy_root(p.root_pa);
            }
        }
        p.root_pa = r.root_pa;
        match crate::mm::heap::kmalloc(4) {
            Ok(rc) => {
                let rcp = rc as *mut u32;
                *rcp = 1;
                p.root_refcount = rcp;
            }
            Err(e) => return e.as_i64(),
        }
        p.entry = r.entry;
        p.ustack = argv_sp;
        p.heap_brk = r.heap_brk;
        p.mmap_brk = 0x2000_0000;
        p.readdir_ino = 0;
        p.readdir_idx = 0;
        p.readdir_active = false;
        p.readdir_fs = crate::fs::vfs::Fs::None;
        p.ring = if r.ring == 1 {
            proc::PROC_RING_ROOT
        } else {
            proc::PROC_RING_USER
        };
        tf.sepc = reg_truncate(r.entry.wrapping_sub(4));
        tf.sp = reg_truncate(argv_sp);
        tf.a0 = reg_truncate(argc as u64);
        tf.a1 = reg_truncate(argv_sp + 8);
        tf.a2 = reg_truncate(envp_sp);
        tf.sstatus =
            reg_truncate(crate::arch::regs::SSTATUS_SPIE | crate::arch::regs::SSTATUS_FS_INITIAL);
        #[cfg(target_pointer_width = "64")]
        {
            tf.satp = crate::arch::regs::SATP_MODE_SV39 | (r.root_pa >> 12);
        }
        #[cfg(target_pointer_width = "32")]
        {
            tf.satp = crate::arch::bits::SATP_MODE_SV32
                | (((r.root_pa >> 12) & 0x3FF_FFFF) as crate::arch::trap_frame::Reg);
        }
        argc as i64
    }
}

/// # Safety
///
/// Call only from handler::handle's syscall path with a live trap frame;
/// the parent is the current process on this hart.
///
/// Fork-race fix (todo P1 #1): `create_user` now leaves the child in
/// `ProcState::Creating` and does NOT enqueue it, so no work-stealing hart
/// can pick the child up while its fds/signal handlers/cwd/trap frame are
/// still being copied below. All inherited state is copied FIRST; only then
/// does `publish_ready` atomically flip the child to Ready and place it on
/// the runqueue of this hart.
pub unsafe fn sys_fork(tf: &mut TrapFrame) -> i64 {
    // SAFETY: parent is this hart's current process; the refcount cell is
    // a live heap u32 owned by the shared root table; by_pid() takes
    // proc_list_lock for the child lookup before the field copies, and
    // publish_ready re-takes it for the Ready+enqueue publication, so the
    // child is never observable half-initialized.
    unsafe {
        let parent = proc::current();
        let parent_pid = parent.pid;
        let new_pid = proc::alloc_pid();

        let mut child_tf = *tf;
        child_tf.a0 = 0;
        child_tf.sepc = tf.sepc.wrapping_add(4);

        let refcount = if parent.root_refcount.is_null() {
            match crate::mm::heap::kmalloc(4) {
                Ok(p) => {
                    let rc = p as *mut u32;
                    *rc = 1;
                    parent.root_refcount = rc;
                    rc
                }
                Err(e) => return e.as_i64(),
            }
        } else {
            parent.root_refcount
        };
        *refcount += 1;

        let ring = parent.ring;
        let result = proc::create_user(
            parent.entry,
            parent.ustack,
            parent.root_pa,
            new_pid,
            parent_pid,
            parent.heap_brk,
            ring,
            tf.a0 as usize,
            parent.ustack,
            refcount,
        );
        match result {
            Ok(()) => {
                let child = match proc::by_pid(new_pid) {
                    Some(p) => p,
                    // by_pid right after create_user cannot fail (the node is
                    // linked under the same lock); if it ever did, the child
                    // would leak in Creating — never runnable, still reaped
                    // as a has_child match by the parent's wait — instead of
                    // running with a half-built state as before.
                    None => return new_pid as i64,
                };
                // Full state copy while the child is Creating (not on any
                // runqueue): no other hart can touch it here.
                child.fds = parent.fds;
                child.signal_handlers = parent.signal_handlers;
                child.signal_mask = parent.signal_mask;
                child.cwd = parent.cwd;
                child.cwd_len = parent.cwd_len;
                child.mmap_brk = parent.mmap_brk;
                child.umask = parent.umask;
                child.tf = child_tf;
                // Atomic publication: Creating → Ready + enqueue. From this
                // point the child is fully initialized and may be stolen.
                if !proc::publish_ready(new_pid) {
                    return Errno::Again.as_i64();
                }
                new_pid as i64
            }
            Err(e) => {
                *refcount -= 1;
                e.as_i64()
            }
        }
    }
}

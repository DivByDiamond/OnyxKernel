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
        // Always close the executable token, including stat/allocation/read errors.
        let loaded = (|| {
            let mut size = 0u32;
            vfs::stat(token, &mut size)?;
            if size == 0 {
                return Err(Errno::Inval);
            }
            let img = crate::mm::heap::kmalloc(size as usize)?;
            let result = (|| {
                let mut offset = 0;
                while offset < size {
                    let read = vfs::read(token, img.add(offset as usize), size - offset)?;
                    if read == 0 {
                        return Err(Errno::Inval);
                    }
                    offset += read;
                }
                proc::onx::load(img, size as usize)
            })();
            crate::mm::heap::kfree(img);
            result
        })();
        let _ = vfs::close(token);
        let r = match loaded {
            Ok(r) => r,
            Err(e) => return e.as_i64(),
        };
        let prepared = match proc::onx::PreparedExec::prepare(r, cur_ring) {
            Ok(prepared) => prepared,
            Err(e) => return e.as_i64(),
        };
        let r = prepared.image();
        let uid = proc::current().uid as u64;
        let (argc, argv_sp) =
            proc::onx::copy_argv_envp_to_stack(r.root_pa, r.ustack, argv, envp, r.entry, uid);
        // RISC-V exec ABI stack layout built above:
        //   [argc][argv...][NULL][envp...][NULL][auxv][strings]
        // so envp starts right after argv's NULL terminator.
        let envp_sp = argv_sp + 8 + (argc as u64 + 1) * 8;
        // Commit only after loading, permission checks, allocation and stack setup.
        // No error return is allowed after close-on-exec descriptors are closed.
        let (r, new_refcount) = prepared.commit();
        for i in 0..proc::PROC_MAX_FDS {
            let fd = proc::current().fds[i];
            if fd.used && fd.cloexec {
                let token = crate::fs::vfs::fd_token(i, fd.epoch);
                let _ = crate::fs::vfs::close(token);
            }
        }
        let p = proc::current();
        let old_root = p.root_pa;
        let old_refcount = p.root_refcount;
        p.root_pa = r.root_pa;
        p.root_refcount = new_refcount;
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
        // Shared roots use the same atomic release protocol as process exit.
        if old_root != 0 && (old_refcount.is_null() || proc::dec_root_refcount(old_refcount)) {
            if !old_refcount.is_null() {
                crate::mm::heap::kfree(old_refcount.cast());
            }
            crate::mm::vmm::destroy_root(old_root);
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
        core::sync::atomic::AtomicU32::from_ptr(refcount)
            .fetch_add(1, core::sync::atomic::Ordering::Relaxed);

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
                // The parent still owns a reference, so this cannot be the last.
                let _ = proc::dec_root_refcount(refcount);
                e.as_i64()
            }
        }
    }
}

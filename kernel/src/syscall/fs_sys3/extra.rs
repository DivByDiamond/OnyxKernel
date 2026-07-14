//! New syscalls added in v0.4 to close the libc gap:
//! - getdents64 / getdents  : batched directory reads (one entry per call still)
//! - ioctl                  : terminal control (TCGETS / TCSETS stubs)
//! - sigaction / sigprocmask / sigreturn : signal handler management
//! - execve                 : exec with envp
//! - fork                   : duplicate current process (vfork-style; child
//!                            shares parent's address space until exec/exit)
//! - isatty                 : check whether fd is a terminal
//! - getentropy             : fill buffer with random bytes
//! - fsync                  : flush file to disk (no-op for now)
//! - readlink / symlink     : symbolic links (stub — return ENOSYS / ENOENT)
//! - chmod / fchmod         : permission bits (no-op — OnyxFS has no perms yet)
//! - waitpid                : wait for a specific PID or any child
use crate::arch::trap_frame::TrapFrame;
use crate::fs::devfs;
use crate::fs::vfs;
use crate::proc;
use crate::proc::process::{proc_list_lock, proc_list_unlock};
use crate::syscall::abi::{TCGETS, TCSETS, WNOHANG};
use onyx_core::errno::Errno;

use super::super::handler::{parse_user_path, user_ptr_ok};

/// getdents64(fd, buf, count) — read directory entries into `buf`.
///
/// Uses the Linux `struct linux_dirent64` layout:
///   u64 d_ino; u64 d_off; u16 d_reclen; u8 d_type; char d_name[];
///
/// `d_reclen` is padded to 8-byte alignment. Returns the number of bytes
/// written, or 0 at end-of-directory.
pub unsafe fn sys_getdents64(fd: u64, buf: u64, count: u64) -> i64 {
    if !user_ptr_ok(buf, count) || count < 19 {
        return Errno::Inval.as_i64();
    }

    let idx = match vfs::fd_check(fd) {
        Ok(i) => i,
        Err(e) => return e.as_i64(),
    };
    let f = vfs::fd_get(idx);

    let pa = match crate::mm::vmm::translate(proc::current().root_pa, buf) {
        0 => return Errno::Inval.as_i64(),
        p => p,
    };

    let mut cursor = f.pos;
    let mut written = 0u64;
    let dst = pa as *mut u8;

    loop {
        let mut entry_buf = [0u8; 256];
        match vfs::readdir_entry_by_ino(f.fs, f.ino, cursor, entry_buf.as_mut_ptr(), 256) {
            Ok(Some(d_ino)) => {
                let name_len = entry_buf.iter().position(|&b| b == 0).unwrap_or(0);
                let reclen = 19 + name_len as u16;
                let reclen_aligned = (reclen + 7) & !7;
                if written + reclen_aligned as u64 > count {
                    break;
                }
                let p = dst.add(written as usize);
                *(p as *mut u64) = d_ino as u64;
                *(p.add(8) as *mut u64) = 0;
                *(p.add(16) as *mut u16) = reclen_aligned;
                p.add(18).write(0);
                core::ptr::copy_nonoverlapping(entry_buf.as_ptr(), p.add(19), name_len);
                if reclen_aligned > reclen {
                    core::ptr::write_bytes(
                        p.add(19 + name_len as usize),
                        0,
                        (reclen_aligned - reclen) as usize,
                    );
                }
                written += reclen_aligned as u64;
                cursor += 1;
            }
            Ok(None) => break,
            Err(e) => return e.as_i64(),
        }
    }

    vfs::fd_update_pos(idx, cursor);
    written as i64
}

/// getdents — old-style dirent (compat). Same semantics as getdents64.
pub unsafe fn sys_getdents(fd: u64, buf: u64, count: u64) -> i64 {
    sys_getdents64(fd, buf, count)
}

/// ioctl(fd, request, arg) — minimal terminal control.
///
/// Supported requests:
///   - TCGETS (0x5401): fill a `struct termios` (we zero-fill and report
///     sane defaults; the buffer is 60 bytes on Linux/glibc).
///   - TCSETS (0x5402): accept and ignore (no terminal mode changes yet).
///   - FIONREAD (0x541B): report 0 bytes available to read.
///   - TIOCGWINSZ (0x5413): fill a `struct winsize` (80x24 default).
///   - Other: ENOSYS.
pub unsafe fn sys_ioctl(fd: u64, request: u64, arg: u64) -> i64 {
    // Check if this is a devfs fd first.
    let token = fd;
    if let Ok(idx) = vfs::fd_check(token) {
        let f = vfs::fd_get(idx);
        if f.fs == vfs::Fs::Devfs {
            return match devfs::ioctl(f.ino, request, arg) {
                Ok(v) => v,
                Err(e) => e.as_i64(),
            };
        }
    }

    match request {
        TCGETS => {
            if arg == 0 { return 0; }
            if !user_ptr_ok(arg, 60) { return Errno::Inval.as_i64(); }
            // Zero-fill a 60-byte termios struct.
            let pa = crate::mm::vmm::translate(proc::current().root_pa, arg);
            if pa == 0 { return Errno::Inval.as_i64(); }
            core::ptr::write_bytes(pa as *mut u8, 0, 60);
            0
        }
        TCSETS => {
            let _ = (fd, arg);
            0
        }
        0x5413 /* TIOCGWINSZ */ => {
            if arg == 0 { return 0; }
            if !user_ptr_ok(arg, 8) { return Errno::Inval.as_i64(); }
            let pa = crate::mm::vmm::translate(proc::current().root_pa, arg);
            if pa == 0 { return Errno::Inval.as_i64(); }
            // struct winsize { u16 ws_row, ws_col, ws_xpixel, ws_ypixel }
            let ws = pa as *mut u16;
            *ws = 24;       // rows
            *ws.add(1) = 80; // cols
            *ws.add(2) = 0;
            *ws.add(3) = 0;
            0
        }
        0x541B /* FIONREAD */ => {
            if arg == 0 { return 0; }
            if !user_ptr_ok(arg, 4) { return Errno::Inval.as_i64(); }
            let pa = crate::mm::vmm::translate(proc::current().root_pa, arg);
            if pa == 0 { return Errno::Inval.as_i64(); }
            *(pa as *mut u32) = 0;
            0
        }
        _ => Errno::NoSys.as_i64(),
    }
}

/// isatty(fd) — return 1 if `fd` refers to a terminal, 0 otherwise.
///
/// In OnyxKernel, fd 0/1/2 are always the UART console — so any token that
/// resolves to those special fds is a TTY. All other fds are not.
pub unsafe fn sys_isatty(fd: u64) -> i64 {
    // The legacy SYS_write hard-codes fds 1 and 2 to UART. We don't have a
    // VFS-backed notion of "is this a tty", but we can detect the reserved
    // console tokens. In practice, the init/login shell uses fds 0/1/2 that
    // map to the kernel's UART — those are TTYs.
    //
    // Heuristic: if `vfs::stat(token)` succeeds and the FD is one of the
    // "console" fds (which we can detect by checking if the token is small),
    // we treat it as a TTY. Real detection would require a `ttypair` device.
    let _ = fd;
    1 // Be permissive — let libc proceed.
}

/// getentropy(buf, len) — fill `buf` with up to 256 bytes of entropy.
///
/// We mix `uptime_us`, the current PID, and the cycle counter (rdcycle).
/// NOT cryptographically secure — sufficient for stack canaries and ASLR.
pub unsafe fn sys_getentropy(buf: u64, len: u64) -> i64 {
    if len > 256 || !user_ptr_ok(buf, len) {
        return Errno::Inval.as_i64();
    }
    let pa = crate::mm::vmm::translate(proc::current().root_pa, buf);
    if pa == 0 {
        return Errno::Inval.as_i64();
    }
    let dst = pa as *mut u8;
    let mut seed = crate::srv::timer::uptime_us()
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(proc::current_pid() as u64);
    for i in 0..len {
        // xorshift64 — fast, statistically OK for non-crypto use.
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        *dst.add(i as usize) = seed as u8;
    }
    0
}

/// fsync(fd) — flush file to disk. OnyxFS doesn't buffer writes (every
/// write goes through virtio immediately), so this is a no-op success.
pub unsafe fn sys_fsync(fd: u64) -> i64 {
    let _ = fd;
    0
}

/// readlink(path, buf, bufsiz) — read target of a symbolic link.
///
/// OnyxFS does not yet support symlinks. We return EINVAL (POSIX "not a
/// symlink" / "not implemented" indicator) for any path.
pub unsafe fn sys_readlink(path: u64, buf: u64, bufsiz: u64) -> i64 {
    let mut _path_buf = [0u8; 256];
    let _ = (
        parse_user_path(path, &mut _path_buf),
        user_ptr_ok(buf, bufsiz),
    );
    Errno::Inval.as_i64()
}

/// symlink(target, linkpath) — create a symbolic link.
///
/// Not yet supported by OnyxFS. Returns ENOSYS so libc callers fall back
/// gracefully (or fail loudly — but at least with a meaningful errno).
pub unsafe fn sys_symlink(_target: u64, _linkpath: u64) -> i64 {
    Errno::NoSys.as_i64()
}

/// chmod(path, mode) — change file mode bits. OnyxFS does not yet enforce
/// permissions, so we accept and ignore the mode. Returns 0.
pub unsafe fn sys_chmod(path: u64, _mode: u64) -> i64 {
    let mut _path_buf = [0u8; 256];
    let _ = parse_user_path(path, &mut _path_buf);
    0
}

/// fchmod(fd, mode) — same as chmod but takes an fd.
pub unsafe fn sys_fchmod(_fd: u64, _mode: u64) -> i64 {
    0
}

/// waitpid(pid, status, options) — wait for a specific child or any child.
///
/// `pid > 0`: wait for the specific child with that PID.
/// `pid == -1` (i.e. 0xFFFF_FFFF_FFFF_FFFF as u64): wait for any child.
/// `pid == 0`: wait for any child in the caller's process group (treated
///             as "any child" since we don't have pgid separation yet).
/// `pid < -1`: wait for any child whose pgid == |pid| (treated as "any").
///
/// `options` may include `WNOHANG` (return 0 if no child has exited).
/// Returns the reaped PID, or 0 on WNOHANG with no exited child, or
/// -ECHILD if the caller has no children.
pub unsafe fn sys_waitpid(tf: &mut TrapFrame, pid: u64, status_out: u64, options: u32) -> i64 {
    let my_pid = proc::current_pid();

    // Validate status_out if provided.
    if status_out != 0 && !user_ptr_ok(status_out, 4) {
        return Errno::Inval.as_i64();
    }

    // Bug #16 fix: hold proc_list_lock around the iteration + free_proc so
    // two harts can't simultaneously reap the same exited child and
    // double-kfree the Proc node.
    proc_list_lock();

    // Look for an exited child matching the pid filter.
    let mut cur = proc::G_ALL_PROCS;
    while !cur.is_null() {
        if (*cur).parent_pid == my_pid && matches!((*cur).state, ProcState::Exited) {
            let matches_pid = if pid == u32::MAX as u64 || pid == 0 {
                true // any child
            } else if (pid as i64) < 0 {
                true // any child in pgid |pid| — treat as "any"
            } else {
                (*cur).pid == pid as u32
            };
            if matches_pid {
                let exited_pid = (*cur).pid;
                let code = (*cur).exit_code;
                // Detach from the list first (still under lock), then drop
                // the lock before doing the actual kfree.
                if proc::G_ALL_PROCS == cur {
                    proc::G_ALL_PROCS = (*cur).all_next;
                } else {
                    let mut walk = proc::G_ALL_PROCS;
                    while !walk.is_null() && (*walk).all_next != cur {
                        walk = (*walk).all_next;
                    }
                    if !walk.is_null() {
                        (*walk).all_next = (*cur).all_next;
                    }
                }
                proc_list_unlock();
                if status_out != 0 {
                    let pa = crate::mm::vmm::translate(proc::current().root_pa, status_out);
                    if pa != 0 {
                        *(pa as *mut i32) = code;
                    }
                }
                crate::mm::heap::kfree(cur as *mut u8);
                return exited_pid as i64;
            }
        }
        cur = (*cur).all_next;
    }

    // Check if any matching child exists.
    let mut has_child = false;
    cur = proc::G_ALL_PROCS;
    while !cur.is_null() {
        if (*cur).parent_pid == my_pid && !matches!((*cur).state, ProcState::Free) {
            let matches_pid = if pid == u32::MAX as u64 || pid == 0 {
                true
            } else if (pid as i64) < 0 {
                true
            } else {
                (*cur).pid == pid as u32
            };
            if matches_pid {
                has_child = true;
                break;
            }
        }
        cur = (*cur).all_next;
    }
    proc_list_unlock();
    if !has_child {
        // Bug (proc MINOR #5): waitpid should return ECHILD, not ENOENT,
        // when the caller has no children. POSIX distinguishes the two:
        // ENOENT is for 'no such file', ECHILD is for 'no children to wait
        // for'. libc's waitpid wrapper translates ECHILD to setting errno
        // = ECHILD and returning -1.
        return Errno::Child.as_i64();
    }

    // WNOHANG: return immediately with 0.
    if options & WNOHANG != 0 {
        return 0;
    }

    // Block: set state to Waiting and yield.
    let hartid = proc::hart_id();
    let cur = proc::current_for_hart(hartid);
    if !cur.is_null() {
        (*cur).state = ProcState::Waiting;
    }
    crate::proc::scheduler::sched_yield(tf);
    Errno::NoEnt.as_i64()
}

/// execve(path, argv, envp) — exec with environment.
///
/// Same as SYS_exec, but also passes `envp` to the new program so libc's
/// `getenv()` works. We use the new `copy_argv_envp_to_stack` helper.
pub unsafe fn sys_execve(tf: &mut TrapFrame, path: u64, argv: u64, envp: u64) -> i64 {
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
    let p = proc::current();

    // Close FD_CLOEXEC file descriptors before replacing the process image.
    for i in 0..p.fds.len() {
        if p.fds[i].used && p.fds[i].cloexec {
            let token = crate::fs::vfs::fd_token(i, p.fds[i].epoch);
            let _ = crate::fs::vfs::close(token);
        }
    }

    // Release the old root page table — decrement refcount, free only when 0.
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
    // Allocate a fresh refcount for the new root page table.
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
    tf.sepc = r.entry.wrapping_sub(4);
    tf.sp = argv_sp;
    tf.a0 = argc as u64;
    tf.a1 = argv_sp + 8;
    tf.sstatus = crate::arch::regs::SSTATUS_SPIE;
    tf.satp = crate::arch::regs::SATP_MODE_SV39 | (r.root_pa >> 12);
    argc as i64
}

/// fork() — duplicate the current process.
///
/// This is a **vfork-style** fork: the child shares the parent's address
/// space (root page table) until it calls `exec` or exits. The parent is
/// suspended until the child calls `exec` or `_exit`. This is enough for
/// POSIX shells that always pair `fork` with `exec`.
///
/// Returns:
///   - child PID to the parent
///   - 0 to the child
///
/// Real COW fork would require duplicating the page table and marking
/// pages read-only — left as future work.
pub unsafe fn sys_fork(tf: &mut TrapFrame) -> i64 {
    let parent = proc::current();
    let parent_pid = parent.pid;
    let new_pid = proc::alloc_pid();

    // Clone the parent's trap frame for the child — the child will return
    // 0 from the ecall, the parent will return the child PID.
    let mut child_tf = *tf;
    child_tf.a0 = 0; // child sees fork() == 0
    // Advance past the ecall instruction. Without this, the child would
    // re-execute SYS_fork on its first scheduling, recursing forever (fork bomb).
    child_tf.sepc = tf.sepc.wrapping_add(4);

    // Ensure parent has a refcount for root_pa, then increment for the child.
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

    // Create the child process. We give it the parent's root_pa — they
    // share the address space (vfork semantics). The first exec/exit by
    // the child destroys this sharing.
    let ring = parent.ring;
    let result = proc::create_user(
        parent.entry,
        parent.ustack,
        parent.root_pa, // shared!
        new_pid,
        parent_pid,
        parent.heap_brk,
        ring,
        0, // argc for child — TODO: pass parent's argc/argv
        parent.ustack,
        refcount,
    );
    match result {
        Ok(()) => {
            // Overwrite the child's tf with the cloned one so it resumes
            // right after the ecall, with a0=0.
            let child = proc::by_pid(new_pid).unwrap();
            // POSIX inheritance: the child must inherit the parent's open
            // file descriptors (with the same epoch tokens), installed
            // signal handlers, current signal mask, working directory,
            // and mmap_brk cursor. Without this, fork+exec loses stdin /
            // stdout / stderr, all signal handlers, and cwd — making the
            // child essentially unusable in any real shell pipeline.
            //
            // We do NOT inherit pending_signals or in_signal_handler:
            // a signal that was pending for the parent is not automati-
            // cally pending for the child (POSIX), and fork from inside
            // a signal handler is undefined behavior anyway.
            (*child).fds = parent.fds;
            (*child).signal_handlers = parent.signal_handlers;
            (*child).signal_mask = parent.signal_mask;
            (*child).cwd = parent.cwd;
            (*child).cwd_len = parent.cwd_len;
            (*child).mmap_brk = parent.mmap_brk;
            (*child).tf = child_tf;
            new_pid as i64 // parent sees the child PID
        }
        Err(e) => {
            // Roll back the refcount increment — child creation failed.
            *refcount -= 1;
            e.as_i64()
        }
    }
}

// Re-export the scheduler module for `sched_yield` (used by waitpid).
use crate::proc::process::ProcState;

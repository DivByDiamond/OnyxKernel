use crate::arch::trap_frame::{TrapFrame, reg_widen};
use crate::proc;
use crate::syscall::abi::*;
use onyx_core::errno::Errno;

use super::acl;

const USER_BASE: u64 = 0x10000;
#[cfg(target_pointer_width = "64")]
const USER_TOP: u64 = 0x4000_0000;
#[cfg(target_pointer_width = "32")]
const USER_TOP: u64 = 0x8000_0000;

pub fn user_ptr_ok(p: u64, len: u64) -> bool {
    p >= USER_BASE && p.checked_add(len).is_some_and(|end| end <= USER_TOP)
}

/// # Safety
///
/// `out` must be valid for writes of 256 bytes; `path` is an untrusted user
/// value that is validated internally (range check plus per-page mapping)
/// before any byte is read.
pub unsafe fn parse_user_path(path: u64, out: &mut [u8; 256]) -> Option<usize> {
    // SAFETY: the scan/copy below only touches [path, path+max_len), and
    // that whole range was just validated by check_user_range; `out`
    // validity is the caller's contract documented above.
    unsafe {
        if !(USER_BASE..USER_TOP).contains(&path) {
            return None;
        }
        // Cap the validated window at how much room this pointer actually
        // has before USER_TOP, not a flat 256: a short string can legally
        // sit within 256 bytes of the top of the user stack (e.g. an argv
        // entry), and demanding a full 256-byte window there rejected it
        // even though every byte of the string itself was in range.
        let max_len = core::cmp::min(256u64, USER_TOP - path) as usize;
        if crate::mm::vmm::check_user_range(
            crate::proc::current().root_pa,
            path,
            max_len as u64,
            false,
        )
        .is_err()
        {
            return None;
        }
        let mut len = 0usize;
        let p = path as *const u8;
        while len < max_len && *p.add(len) != 0 {
            len += 1;
        }
        core::ptr::copy_nonoverlapping(p, out.as_mut_ptr(), len);
        Some(len)
    }
}

/// # Safety
///
/// `tf` must be this hart's live trap frame of the interrupted context;
/// called exactly once per syscall from the trap path with a current process
/// set and after the ACL check inside this function.
pub unsafe fn handle(tf: &mut TrapFrame) -> i64 {
    // SAFETY: the only unsafe operations are the calls to the sys_* handlers,
    // each of which validates its own user-pointer arguments; a0..a5 are
    // widened copies of the trap-frame registers and `tf` is live per the
    // contract above.
    unsafe {
        // Syscall arguments are widened to u64 on every target so the ABI
        // layer stays pointer-width independent (on rv32 the raw registers
        // are u32 and zero-extend into the canonical u64 ABI values).
        let nr = reg_widen(tf.a7);
        let a0 = reg_widen(tf.a0);
        let a1 = reg_widen(tf.a1);
        let a2 = reg_widen(tf.a2);
        let a3 = reg_widen(tf.a3);
        let a4 = reg_widen(tf.a4);
        let a5 = reg_widen(tf.a5);
        let cur_ring = proc::current_ring();

        if !acl::syscall_allowed(nr, cur_ring) {
            return Errno::translate_syscall_result(Errno::Perm.as_i64());
        }

        // Every arm below returns either a non-negative success value or a
        // negative `Errno::X.as_i64()` (the internal -1..-20 ordinal, NOT a
        // POSIX errno). `translate_syscall_result` is the single point that
        // converts that ordinal to the POSIX-numbered value userspace's
        // errno.h documents and expects (see onyx_core::errno::Errno::to_posix
        // for why this boundary exists — historically syscalls returned the
        // raw ordinal straight to userspace, which desynced from
        // libonyxc/include/io/errno.h's Linux/glibc numbering).
        let raw = match nr {
            SYS_write => crate::syscall::fs_sys::sys_write(tf, a0, a1, a2),
            SYS_read => crate::syscall::fs_sys::sys_read(tf, a0, a1, a2),
            SYS_exit => crate::syscall::proc_sys::sys_exit(a0),
            SYS_yield => crate::syscall::proc_sys::sys_yield(),
            SYS_getpid => crate::syscall::proc_sys::sys_getpid(),
            SYS_open => crate::syscall::fs_sys::sys_open(a0, a1, a2),
            SYS_close => crate::syscall::fs_sys::sys_close(a0),
            SYS_lseek => crate::syscall::fs_sys::sys_lseek(a0, a1 as i64, a2 as u32),
            SYS_stat => crate::syscall::fs_sys::sys_stat(a0, a1),
            SYS_exec => crate::syscall::fs_sys2::sys_exec(tf, a0, a1),
            SYS_sbrk => crate::syscall::fs_sys2::sys_sbrk(a0 as i64),
            SYS_spawn => crate::syscall::proc_sys::sys_spawn(tf, a0, a1, a2 as u8),
            SYS_wait => crate::syscall::proc_sys::sys_wait(tf, a0),
            SYS_readdir => crate::syscall::fs_sys2::sys_readdir(a0, a1, a2),
            SYS_getring => crate::syscall::ring_sys::sys_getring(),
            SYS_dropring => crate::syscall::ring_sys::sys_dropring(a0 as u8),
            SYS_snapshot_create => crate::syscall::snap_sys::sys_snapshot_create(a0),
            SYS_snapshot_rollback => crate::syscall::snap_sys::sys_snapshot_rollback(a0 as u32),
            SYS_snapshot_list => crate::syscall::snap_sys::sys_snapshot_list(a0, a1),
            SYS_kill => crate::syscall::proc_sys::sys_kill(a0 as u32, a1 as u32),
            SYS_sigmask => crate::syscall::proc_sys::sys_sigmask(a0 as u32, a1 as u32),
            SYS_write_fd => crate::syscall::fs_sys2::sys_write_fd(a0, a1, a2),
            SYS_create => crate::syscall::fs_sys2::sys_create(a0, a1, a2),
            SYS_mkdir => crate::syscall::fs_sys2::sys_mkdir(a0),
            SYS_chan_create => crate::syscall::ipc_sys::sys_chan_create(),
            SYS_chan_create_named => crate::syscall::ipc_sys::sys_chan_create_named(a0),
            SYS_chan_open => crate::syscall::ipc_sys::sys_chan_open(a0),
            SYS_chan_connect => crate::syscall::ipc_sys::sys_chan_connect(a0 as u32),
            SYS_chan_send => crate::syscall::ipc_sys::sys_chan_send(tf, a0 as u32, a1, a2),
            SYS_chan_recv => crate::syscall::ipc_sys::sys_chan_recv(tf, a0 as u32, a1, a2),
            SYS_chan_close => crate::syscall::ipc_sys::sys_chan_close(a0 as u32),
            SYS_brk => crate::syscall::fs_sys3::sys_brk(a0),
            SYS_mmap => crate::syscall::fs_sys3::sys_mmap(a0, a1, a2, a3, a4, a5),
            SYS_munmap => crate::syscall::fs_sys3::sys_munmap(a0, a1),
            SYS_dup => crate::syscall::fs_sys3::sys_dup(a0),
            SYS_pipe => crate::syscall::fs_sys3::sys_pipe(a0),
            SYS_unlink => crate::syscall::fs_sys3::sys_unlink(a0),
            SYS_rename => crate::syscall::fs_sys3::sys_rename(a0, a1),
            SYS_chdir => crate::syscall::fs_sys3::sys_chdir(a0),
            SYS_getcwd => crate::syscall::fs_sys3::sys_getcwd(a0, a1),
            SYS_truncate => crate::syscall::fs_sys3::sys_truncate(a0),
            SYS_access => crate::syscall::fs_sys3::sys_access(a0, a1),
            SYS_gettimeofday => crate::syscall::fs_sys3::sys_gettimeofday(a0),
            SYS_fcntl => crate::syscall::fs_sys::sys_fcntl(a0, a1 as u32, a2),
            SYS_getuid => crate::syscall::fs_sys3::sys_getuid(),
            SYS_getgid => crate::syscall::fs_sys3::sys_getgid(),
            SYS_umask => crate::syscall::fs_sys3::sys_umask(a0),
            SYS_utimens => crate::syscall::fs_sys3::sys_utimens(a0, a1),
            SYS_uname => crate::syscall::fs_sys3::sys_uname(a0),
            SYS_nanosleep => crate::syscall::fs_sys3::sys_nanosleep(a0, a1),
            SYS_fstat => crate::syscall::fs_sys::sys_fstat(a0, a1),
            SYS_waitpid => crate::syscall::fs_sys3::sys_waitpid(tf, a0, a1, a2 as u32),
            SYS_getdents64 => crate::syscall::fs_sys3::sys_getdents64(a0, a1, a2),
            SYS_ioctl => crate::syscall::fs_sys3::sys_ioctl(a0, a1, a2),
            SYS_mprotect => crate::syscall::fs_sys3::sys_mprotect(a0, a1, a2),
            SYS_sigaction => match crate::syscall::fs_sys3::sys_sigaction_impl(a0 as u32, a1, a2) {
                Ok(()) => 0,
                Err(e) => e.as_i64(),
            },
            SYS_sigprocmask => {
                match crate::syscall::fs_sys3::sys_sigprocmask_impl(a0 as u32, a1, a2) {
                    Ok(()) => 0,
                    Err(e) => e.as_i64(),
                }
            }
            SYS_sigreturn => {
                crate::syscall::fs_sys3::sys_sigreturn_impl(tf);
                0
            }
            SYS_execve => crate::syscall::fs_sys3::sys_execve(tf, a0, a1, a2),
            SYS_getppid => crate::syscall::fs_sys3::sys_getppid(),
            SYS_setpgid => crate::syscall::fs_sys3::sys_setpgid(a0, a1),
            SYS_setsid => crate::syscall::fs_sys3::sys_setsid(),
            SYS_getpgid => crate::syscall::fs_sys3::sys_getpgid(a0),
            SYS_fork => crate::syscall::fs_sys3::sys_fork(tf),
            SYS_clock_gettime => crate::syscall::fs_sys3::sys_clock_gettime(a0, a1),
            SYS_clock_getres => crate::syscall::fs_sys3::sys_clock_getres(a0, a1),
            SYS_isatty => crate::syscall::fs_sys3::sys_isatty(a0),
            SYS_getentropy => crate::syscall::fs_sys3::sys_getentropy(a0, a1),
            SYS_setuid => crate::syscall::fs_sys3::sys_setuid(a0),
            SYS_setgid => crate::syscall::fs_sys3::sys_setgid(a0),
            SYS_fsync => crate::syscall::fs_sys3::sys_fsync(a0),
            SYS_truncate2 => crate::syscall::fs_sys3::sys_truncate2(a0, a1),
            SYS_ftruncate => crate::syscall::fs_sys3::sys_ftruncate(a0, a1),
            SYS_readlink => crate::syscall::fs_sys3::sys_readlink(a0, a1, a2),
            SYS_symlink => crate::syscall::fs_sys3::sys_symlink(a0, a1),
            SYS_chmod => crate::syscall::fs_sys3::sys_chmod(a0, a1),
            SYS_fchmod => crate::syscall::fs_sys3::sys_fchmod(a0, a1),
            SYS_getdents => crate::syscall::fs_sys3::sys_getdents(a0, a1, a2),
            SYS_sched_setaffinity => crate::syscall::proc_sys::sys_sched_setaffinity(a0, a1 as i64),
            SYS_sched_getaffinity => crate::syscall::proc_sys::sys_sched_getaffinity(a0),
            SYS_net_connect => crate::syscall::net_sys::sys_net_connect(a0, a1),
            SYS_net_send => crate::syscall::net_sys::sys_net_send(a0, a1, a2),
            SYS_net_recv => crate::syscall::net_sys::sys_net_recv(a0, a1, a2),
            SYS_net_close => crate::syscall::net_sys::sys_net_close(a0),
            SYS_net_resolve => crate::syscall::net_sys::sys_net_resolve(a0, a1),
            SYS_chown => crate::syscall::fs_sys3::sys_chown(a0, a1 as u32, a2 as u32),
            SYS_fchown => crate::syscall::fs_sys3::sys_fchown(a0, a1 as u32, a2 as u32),
            SYS_mouse_read => crate::syscall::input_sys::sys_mouse_read(
                a0 as *mut crate::syscall::input_sys::MouseEvent,
            ),
            SYS_poll => crate::syscall::poll_sys::sys_poll(tf, a0, a1, a2 as i64),
            _ => Errno::NoSys.as_i64(),
        };
        Errno::translate_syscall_result(raw)
    }
}

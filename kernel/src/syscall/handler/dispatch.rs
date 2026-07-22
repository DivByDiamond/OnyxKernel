use crate::arch::trap_frame::TrapFrame;
use crate::proc;
use crate::syscall::abi::*;
use onyx_core::errno::Errno;

use super::acl;

const USER_BASE: u64 = 0x10000;
#[cfg(target_pointer_width = "64")]
const USER_TOP: u64 = 0x4000_0000;
#[cfg(target_pointer_width = "32")]
const USER_TOP: u64 = 0x8000_0000;

pub(super) fn user_ptr_ok(p: u64, len: u64) -> bool {
    p >= USER_BASE && p.checked_add(len).is_some_and(|end| end <= USER_TOP)
}

pub(super) unsafe fn parse_user_path(path: u64, out: &mut [u8; 256]) -> Option<usize> {
    if !user_ptr_ok(path, 256) {
        return None;
    }
    let mut len = 0usize;
    let p = path as *const u8;
    while len < 256 && *p.add(len) != 0 {
        len += 1;
    }
    core::ptr::copy_nonoverlapping(p, out.as_mut_ptr(), len);
    Some(len)
}

pub unsafe fn handle(tf: &mut TrapFrame) -> i64 {
    let nr = tf.a7;
    let a0 = tf.a0;
    let a1 = tf.a1;
    let a2 = tf.a2;
    let cur_ring = proc::current_ring();

    if !acl::syscall_allowed(nr, cur_ring) {
        return Errno::Perm.as_i64();
    }

    match nr {
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
        SYS_mmap => crate::syscall::fs_sys3::sys_mmap(a0, a1, a2, tf.a3, tf.a4, tf.a5),
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
        SYS_sigprocmask => match crate::syscall::fs_sys3::sys_sigprocmask_impl(a0 as u32, a1, a2) {
            Ok(()) => 0,
            Err(e) => e.as_i64(),
        },
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
        SYS_chown => crate::syscall::fs_sys3::sys_chown(a0, a1 as u32, a2 as u32),
        SYS_fchown => crate::syscall::fs_sys3::sys_fchown(a0, a1 as u32, a2 as u32),
        _ => Errno::NoSys.as_i64(),
    }
}

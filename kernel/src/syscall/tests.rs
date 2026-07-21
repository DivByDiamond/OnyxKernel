use super::abi::*;
use super::handler;

#[test]
fn test_syscall_numbers_unique_and_complete() {
    let all = [
        SYS_write,
        SYS_read,
        SYS_exit,
        SYS_yield,
        SYS_getpid,
        SYS_brk,
        SYS_mmap,
        SYS_open,
        SYS_close,
        SYS_lseek,
        SYS_stat,
        SYS_exec,
        SYS_sbrk,
        SYS_spawn,
        SYS_wait,
        SYS_readdir,
        SYS_getring,
        SYS_dropring,
        SYS_snapshot_create,
        SYS_snapshot_rollback,
        SYS_snapshot_list,
        SYS_kill,
        SYS_sigmask,
        SYS_write_fd,
        SYS_create,
        SYS_mkdir,
        SYS_chan_create,
        SYS_chan_connect,
        SYS_chan_send,
        SYS_chan_recv,
        SYS_chan_close,
        SYS_chan_create_named,
        SYS_chan_open,
        SYS_munmap,
        SYS_dup,
        SYS_pipe,
        SYS_unlink,
        SYS_rename,
        SYS_chdir,
        SYS_getcwd,
        SYS_truncate,
        SYS_access,
        SYS_gettimeofday,
        SYS_fcntl,
        SYS_getuid,
        SYS_getgid,
        SYS_utimens,
        SYS_uname,
        SYS_nanosleep,
        SYS_fstat,
        SYS_waitpid,
        SYS_getdents64,
        SYS_ioctl,
        SYS_mprotect,
        SYS_sigaction,
        SYS_sigprocmask,
        SYS_sigreturn,
        SYS_execve,
        SYS_getppid,
        SYS_setpgid,
        SYS_setsid,
        SYS_getpgid,
        SYS_fork,
        SYS_clock_gettime,
        SYS_clock_getres,
        SYS_isatty,
        SYS_getentropy,
        SYS_setuid,
        SYS_setgid,
        SYS_fsync,
        SYS_truncate2,
        SYS_ftruncate,
        SYS_readlink,
        SYS_symlink,
        SYS_chmod,
        SYS_fchmod,
        SYS_getdents,
        SYS_sched_setaffinity,
        SYS_sched_getaffinity,
        SYS_net_connect,
        SYS_net_send,
        SYS_net_recv,
        SYS_net_close,
        SYS_chown,
        SYS_fchown,
    ];
    let mut seen = [false; 86];
    for &nr in &all {
        assert!(nr >= 1 && nr <= 85, "syscall {} out of range", nr);
        let idx = nr as usize;
        assert!(!seen[idx], "syscall {} duplicated", nr);
        seen[idx] = true;
    }
    for i in 1..=85 {
        assert!(seen[i], "syscall {} missing", i);
    }
}

#[test]
fn test_user_ptr_ok_below_base() {
    assert!(!handler::user_ptr_ok(0, 1));
    assert!(!handler::user_ptr_ok(0xFFFF, 1));
}

#[test]
fn test_user_ptr_ok_at_base() {
    assert!(handler::user_ptr_ok(0x10000, 0));
    assert!(handler::user_ptr_ok(0x10000, 1));
    assert!(handler::user_ptr_ok(0x10000, 4096));
}

#[test]
fn test_user_ptr_ok_near_top() {
    assert!(handler::user_ptr_ok(0x3FFF_FFFF, 1));
    assert!(!handler::user_ptr_ok(0x3FFF_FFFF, 2));
    assert!(handler::user_ptr_ok(0x4000_0000, 0));
    assert!(!handler::user_ptr_ok(0x4000_0000, 1));
}

#[test]
fn test_user_ptr_ok_overflow() {
    assert!(!handler::user_ptr_ok(!0u64, 1));
    assert!(!handler::user_ptr_ok(!0u64 - 10, 20));
}

#[test]
fn test_user_ptr_ok_zero_len() {
    assert!(!handler::user_ptr_ok(0, 0));
    assert!(!handler::user_ptr_ok(0xFFFF, 0));
    assert!(handler::user_ptr_ok(0x10000, 0));
    assert!(handler::user_ptr_ok(0x4000_0000, 0));
}

#[test]
fn test_open_flags() {
    assert_eq!(O_RDONLY, 0);
    assert_eq!(O_WRONLY, 1);
    assert_eq!(O_RDWR, 2);
    assert_eq!(O_ACCMODE, 3);
    assert_eq!(O_CREAT, 0x40);
    assert_eq!(O_EXCL, 0x80);
    assert_eq!(O_TRUNC, 0x200);
    assert_eq!(O_APPEND, 0x400);
    assert_eq!(O_NONBLOCK, 0x800);
    assert_eq!(O_DIRECTORY, 0x10000);
}

#[test]
fn test_seek_constants() {
    assert_eq!(SEEK_SET, 0);
    assert_eq!(SEEK_CUR, 1);
    assert_eq!(SEEK_END, 2);
}

#[test]
fn test_fcntl_constants() {
    assert_eq!(F_DUPFD, 0);
    assert_eq!(F_GETFD, 1);
    assert_eq!(F_SETFD, 2);
    assert_eq!(F_GETFL, 3);
    assert_eq!(F_SETFL, 4);
    assert_eq!(FD_CLOEXEC, 1);
}

#[test]
fn test_ring_constants() {
    assert_eq!(RING_KERNEL, 0);
    assert_eq!(RING_ROOT, 1);
    assert_eq!(RING_USER, 2);
}

#[test]
fn test_signal_constants() {
    assert_eq!(SIGHUP, 1);
    assert_eq!(SIGINT, 2);
    assert_eq!(SIGKILL, 9);
    assert_eq!(SIGSTOP, 19);
    assert_eq!(NSIG, 32);
}

#[test]
fn test_waitpid_flags() {
    assert_eq!(WNOHANG, 1);
    assert_eq!(WUNTRACED, 2);
}

#[test]
fn test_clock_constants() {
    assert_eq!(CLOCK_REALTIME, 0);
    assert_eq!(CLOCK_MONOTONIC, 1);
}

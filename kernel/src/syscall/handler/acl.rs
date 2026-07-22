use crate::proc;
use crate::syscall::abi::*;

pub(super) fn syscall_allowed(nr: u64, ring: u8) -> bool {
    match nr {
        SYS_write
        | SYS_read
        | SYS_exit
        | SYS_yield
        | SYS_getpid
        | SYS_sbrk
        | SYS_open
        | SYS_close
        | SYS_lseek
        | SYS_stat
        | SYS_exec
        | SYS_readdir
        | SYS_getring
        | SYS_dropring
        | SYS_sigmask
        | SYS_write_fd
        | SYS_chan_connect
        | SYS_chan_send
        | SYS_chan_recv
        | SYS_chan_close
        | SYS_chan_open
        | SYS_brk
        | SYS_mmap
        | SYS_munmap
        | SYS_dup
        | SYS_chdir
        | SYS_getcwd
        | SYS_access
        | SYS_gettimeofday
        | SYS_fcntl
        | SYS_getuid
        | SYS_getgid
        | SYS_uname
        | SYS_nanosleep
        | SYS_fstat
        | SYS_getdents64
        | SYS_getdents
        | SYS_ioctl
        | SYS_mprotect
        | SYS_sigaction
        | SYS_sigprocmask
        | SYS_sigreturn
        | SYS_execve
        | SYS_getppid
        | SYS_clock_gettime
        | SYS_clock_getres
        | SYS_isatty
        | SYS_getentropy
        | SYS_waitpid
        | SYS_fork
        | SYS_ftruncate
        | SYS_truncate2
        | SYS_readlink
        | SYS_setsid
        | SYS_getpgid
        | SYS_setpgid
        | SYS_sched_setaffinity
        | SYS_sched_getaffinity
        | SYS_net_connect
        | SYS_net_send
        | SYS_net_recv
        | SYS_net_close
        | SYS_setuid
        | SYS_setgid => true,
        SYS_spawn
        | SYS_wait
        | SYS_snapshot_create
        | SYS_snapshot_rollback
        | SYS_snapshot_list
        | SYS_kill
        | SYS_create
        | SYS_mkdir
        | SYS_chan_create
        | SYS_chan_create_named
        | SYS_unlink
        | SYS_rename
        | SYS_truncate
        | SYS_utimens
        | SYS_pipe
        | SYS_fsync
        | SYS_symlink
        | SYS_chmod
        | SYS_fchmod
        | SYS_chown
        | SYS_fchown => ring <= proc::PROC_RING_ROOT,
        _ => false,
    }
}

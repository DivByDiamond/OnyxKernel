use crate::proc;
use crate::syscall::abi::*;

/// Pure ACL decision (host-testable): same table as `syscall_allowed`, but
/// the caller's uid is passed in explicitly (`None` = unknown) instead of
/// being read from the current process.
fn syscall_allowed_uid(nr: u64, ring: u8, uid: Option<u32>) -> bool {
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
        | SYS_poll
        | SYS_setuid
        | SYS_setgid => true,
        SYS_spawn
        | SYS_wait
        | SYS_snapshot_create
        | SYS_snapshot_rollback
        | SYS_snapshot_list
        | SYS_kill
        | SYS_mkdir
        | SYS_chan_create
        | SYS_chan_create_named
        | SYS_unlink
        | SYS_truncate
        | SYS_utimens
        | SYS_pipe
        | SYS_fsync
        | SYS_symlink
        | SYS_chmod
        | SYS_fchmod
        | SYS_chown
        | SYS_fchown => ring <= proc::PROC_RING_ROOT,
        // ACL rule (todo.md "umask/OnyxFS permissions"): create/rename are
        // allowed for ring <= 1 and for ring-2 processes with uid == 0. This
        // is root self-service: /bin/passwd runs in ring 2 and must recreate
        // /etc/shadow, while root already bypasses the shadow-deny in open
        // (fs_sys/open_close/open.rs). For ordinary ring-2 users the
        // default-deny stays — shadow protection is not weakened; only
        // root's ability to modify its own system files from its own shell
        // changes.
        SYS_create | SYS_rename => ring <= proc::PROC_RING_ROOT || uid.is_some_and(|u| u == 0),
        _ => false,
    }
}

pub(super) fn syscall_allowed(nr: u64, ring: u8) -> bool {
    syscall_allowed_uid(nr, ring, proc::current_opt().map(|p| p.uid))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proc::{PROC_RING_KERNEL, PROC_RING_ROOT, PROC_RING_USER};

    #[test]
    fn test_unprivileged_calls_allowed_in_every_ring() {
        // Baseline user-facing calls work from ring 0, 1 and 2 alike.
        for ring in [PROC_RING_ROOT - 1, PROC_RING_ROOT, PROC_RING_USER] {
            assert!(syscall_allowed_uid(SYS_write, ring, None));
            assert!(syscall_allowed_uid(SYS_read, ring, None));
            assert!(syscall_allowed_uid(SYS_exit, ring, None));
            assert!(syscall_allowed_uid(SYS_open, ring, None));
            assert!(syscall_allowed_uid(SYS_fork, ring, None));
            assert!(syscall_allowed_uid(SYS_net_connect, ring, None));
        }
    }

    #[test]
    fn test_chan_open_available_to_user_space() {
        // Regression: SYS_chan_open was once missing from the ACL and
        // ring-2 processes got a silent deny on channel open.
        for ring in [PROC_RING_KERNEL, PROC_RING_ROOT, PROC_RING_USER] {
            assert!(syscall_allowed_uid(SYS_chan_open, ring, None));
            assert!(syscall_allowed_uid(SYS_chan_connect, ring, None));
            assert!(syscall_allowed_uid(SYS_chan_send, ring, None));
            assert!(syscall_allowed_uid(SYS_chan_recv, ring, None));
            assert!(syscall_allowed_uid(SYS_chan_close, ring, None));
        }
    }

    #[test]
    fn test_ring1_only_calls_denied_at_ring2() {
        // Spawn/kill/fs-mutation calls are root-space (ring <= 1) only.
        let ring1_only = [
            SYS_spawn,
            SYS_wait,
            SYS_kill,
            SYS_mkdir,
            SYS_chan_create,
            SYS_chan_create_named,
            SYS_unlink,
            SYS_truncate,
            SYS_utimens,
            SYS_pipe,
            SYS_symlink,
            SYS_chmod,
            SYS_fchmod,
            SYS_chown,
            SYS_fchown,
            SYS_snapshot_create,
            SYS_snapshot_rollback,
            SYS_snapshot_list,
            SYS_fsync,
        ];
        for &nr in &ring1_only {
            assert!(!syscall_allowed_uid(nr, PROC_RING_USER, Some(1000)));
            assert!(syscall_allowed_uid(nr, PROC_RING_ROOT, Some(0)));
            assert!(syscall_allowed_uid(nr, PROC_RING_KERNEL, Some(0)));
        }
    }

    #[test]
    fn test_create_rename_root_uid_exception() {
        // create/rename: ring <= 1 always; ring 2 only for uid 0 (root
        // self-service for /bin/passwd in ring 2) — never for ordinary users.
        assert!(syscall_allowed_uid(SYS_create, PROC_RING_KERNEL, None));
        assert!(syscall_allowed_uid(SYS_create, PROC_RING_ROOT, Some(1000)));
        assert!(syscall_allowed_uid(SYS_rename, PROC_RING_ROOT, None));
        assert!(syscall_allowed_uid(SYS_create, PROC_RING_USER, Some(0)));
        assert!(syscall_allowed_uid(SYS_rename, PROC_RING_USER, Some(0)));
        assert!(!syscall_allowed_uid(SYS_create, PROC_RING_USER, Some(1000)));
        assert!(!syscall_allowed_uid(SYS_rename, PROC_RING_USER, Some(1000)));
        // Unknown uid (no current process) keeps the default deny at ring 2.
        assert!(!syscall_allowed_uid(SYS_create, PROC_RING_USER, None));
    }

    #[test]
    fn test_poll_available_to_user_space() {
        // poll() is the non-blocking I/O entry point (todo P1 #1) — TUI
        // programs in every ring need it with a timeout.
        for ring in [PROC_RING_KERNEL, PROC_RING_ROOT, PROC_RING_USER] {
            assert!(syscall_allowed_uid(SYS_poll, ring, None));
            assert!(syscall_allowed_uid(SYS_poll, ring, Some(1000)));
        }
    }

    #[test]
    fn test_unknown_syscall_numbers_denied() {
        for &nr in &[0u64, 88, 200, u64::MAX] {
            for ring in [PROC_RING_KERNEL, PROC_RING_ROOT, PROC_RING_USER] {
                assert!(!syscall_allowed_uid(nr, ring, Some(0)));
            }
        }
    }
}

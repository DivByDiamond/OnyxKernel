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
    // Spawn/fs-mutation calls are root-space (ring <= 1) only. kill is
    // deliberately NOT in this list anymore (todo P2 #5): ring 2 may
    // signal its own group (enforced in sys_kill).
    let ring1_only = [
        SYS_spawn,
        SYS_wait,
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
fn test_reboot_root_uid_exception() {
    // reboot: ring <= 1 always; ring 2 only for uid 0 (root's own shell
    // spawning /bin/powerctl). Ordinary users stay denied.
    assert!(syscall_allowed_uid(SYS_reboot, PROC_RING_KERNEL, None));
    assert!(syscall_allowed_uid(SYS_reboot, PROC_RING_ROOT, Some(1000)));
    assert!(syscall_allowed_uid(SYS_reboot, PROC_RING_USER, Some(0)));
    assert!(!syscall_allowed_uid(SYS_reboot, PROC_RING_USER, Some(1000)));
    assert!(!syscall_allowed_uid(SYS_reboot, PROC_RING_USER, None));
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
fn test_network_admin_calls_require_root() {
    for nr in [
        SYS_net_config,
        SYS_udp_bind,
        SYS_udp_sendto,
        SYS_udp_recv,
        SYS_udp_close,
        SYS_net_listen,
        SYS_net_accept,
        SYS_net_unlisten,
        SYS_udp_recvfrom,
        SYS_udp_sendfrom,
    ] {
        assert!(!syscall_allowed_uid(nr, PROC_RING_USER, Some(1000)));
        assert!(!syscall_allowed_uid(nr, PROC_RING_USER, None));
        assert!(syscall_allowed_uid(nr, PROC_RING_USER, Some(0)));
        assert!(syscall_allowed_uid(nr, PROC_RING_ROOT, Some(1000)));
    }
}

#[test]
fn test_unknown_syscall_numbers_denied() {
    for &nr in &[0u64, 96, 200, u64::MAX] {
        for ring in [PROC_RING_KERNEL, PROC_RING_ROOT, PROC_RING_USER] {
            assert!(!syscall_allowed_uid(nr, ring, Some(0)));
        }
    }
}

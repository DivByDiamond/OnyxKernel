use super::*;
use crate::proc::PROC_RING_KERNEL;

#[test]
fn test_chown_owner_may_chown_own_file() {
    assert!(chown_allowed(1000, PROC_RING_ROOT + 1, 1000));
}

#[test]
fn test_chown_non_owner_user_denied() {
    // The whole point of the fix: uid 2001 cannot chown uid 2000's file.
    assert!(!chown_allowed(2001, PROC_RING_ROOT + 1, 2000));
    // Even to itself.
    assert!(!chown_allowed(2001, PROC_RING_ROOT + 1, 2001 + 1));
}

#[test]
fn test_chown_root_uid_allowed_any_target() {
    assert!(chown_allowed(0, PROC_RING_ROOT + 1, 1000));
    assert!(chown_allowed(0, PROC_RING_ROOT + 1, 0));
    assert!(chown_allowed(0, PROC_RING_ROOT + 1, 65535));
}

#[test]
fn test_chown_privileged_ring_allowed() {
    // Kernel ring (0) and root ring (1) bypass the uid match.
    assert!(chown_allowed(1000, PROC_RING_KERNEL, 2000));
    assert!(chown_allowed(1000, PROC_RING_ROOT, 2000));
}

#[test]
fn test_chown_user_ring_cannot_take_ownership() {
    // uid 0 attacker in a USER ring still passes only via uid==0 — the
    // ring alone must not elevate an arbitrary uid... it does in this
    // kernel's threat model (ring is set by the loader), so document
    // the actual policy: ring <= 1 IS privileged regardless of uid.
    assert!(chown_allowed(1000, PROC_RING_ROOT, 2000));
    assert!(!chown_allowed(1000, PROC_RING_ROOT + 1, 2000));
}

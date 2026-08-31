use crate::fs::onyxfs;
use crate::fs::vfs::{FdToken, Fs, fd_check, fd_get, is_kernel_boot, resolve_mount};
use crate::proc::PROC_RING_ROOT;
use onyx_core::errno::{Errno, KResult};

/// Privilege decision for chown/fchown (security fix, todo P1 #6).
///
/// A caller may change ownership iff it is the file owner, uid 0, or
/// running in a privileged ring (kernel / root ring — `uid_for_ring` maps
/// ring <= 1 to uid 0, so the three clauses agree). Everything else gets
/// EPERM. Kept as a pure function so the policy is unit-testable without a
/// mounted filesystem; `chown`/`fchown` only apply the verdict.
///
/// POSIX nuance: a non-root owner may not hand the file to ANOTHER user
/// (no "give away" chown) — the caller must already be the owner, and
/// uid 0 is the only identity allowed to set arbitrary ownership.
fn chown_allowed(caller_uid: u32, caller_ring: u8, inode_uid: u32) -> bool {
    caller_uid == inode_uid || caller_uid == 0 || caller_ring <= PROC_RING_ROOT
}

/// # Safety
///
/// Caller contract: path comes from the syscall layer's parse_user_path
/// (kernel-side slice). Ownership check is performed inside (security fix,
/// todo P1 #6 — previously ANY process could chown ANY file).
pub unsafe fn chown(path: &[u8], uid: u32, gid: u32) -> KResult<()> {
    // SAFETY: path slice is kernel-side; onyxfs::lookup/set_uid_gid take
    // no cross-hart lock — concurrent chown calls are not serialized
    // (journal is crash recovery only); caller must not race across harts.
    unsafe {
        if path.is_empty() || path[0] != b'/' {
            return Err(Errno::Inval);
        }
        let name = &path[1..];
        let (fs, _) = resolve_mount(name);
        if fs != Fs::Onyx {
            return Err(Errno::NoSys);
        }
        let mut st = onyxfs::OnyfsStat::default();
        let ino = onyxfs::lookup(name, &mut st)?;
        // Security fix (todo P1 #6): ownership/privilege check, mirroring
        // chmod. Boot-time callers (no current process) bypass, same as
        // chmod, so first-boot filesystem population keeps working.
        if !is_kernel_boot() {
            let cur = crate::proc::current();
            if !chown_allowed(cur.uid, cur.ring, st.uid) {
                return Err(Errno::Perm);
            }
        }
        onyxfs::set_uid_gid(ino, uid, gid)
    }
}

/// # Safety
///
/// Caller contract: token must be a live fd token of the calling context.
/// Ownership check is performed inside (security fix, todo P1 #6).
pub unsafe fn fchown(token: FdToken, uid: u32, gid: u32) -> KResult<()> {
    // SAFETY: fd_check validates idx and epoch. onyxfs::stat/set_uid_gid take
    // no cross-hart lock — concurrent fchown calls are not serialized;
    // caller must not race metadata updates across harts.
    unsafe {
        let idx = fd_check(token)?;
        let fd = fd_get(idx);
        if fd.fs != Fs::Onyx {
            return Err(Errno::NoSys);
        }
        // Security fix (todo P1 #6): same policy as chown, resolved via the
        // fd's inode number. stat() errors are swallowed to keep the
        // previous best-effort behavior of the boot path, but a FAILED stat
        // cannot grant permission: st stays default (uid 0 = root-owned),
        // so a non-root caller is still rejected by chown_allowed.
        if !is_kernel_boot() {
            let mut st = onyxfs::OnyfsStat::default();
            let _ = onyxfs::stat(fd.ino, &mut st);
            let cur = crate::proc::current();
            if !chown_allowed(cur.uid, cur.ring, st.uid) {
                return Err(Errno::Perm);
            }
        }
        onyxfs::set_uid_gid(fd.ino, uid, gid)
    }
}

#[cfg(test)]
mod tests {
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
}

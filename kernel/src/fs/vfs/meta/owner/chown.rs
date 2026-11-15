use crate::fs::onyxfs;
use crate::fs::vfs::{FdToken, Fs, fd_check, fd_get, is_kernel_boot, stat_target, with_mount};
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
    // SAFETY: stat_target validates the path and filters non-Onyx to ENOSYS;
    // set_uid_gid runs in the resolved mount's singleton context under
    // FS_LOCK (serialization supersedes the old "caller must not race" note).
    unsafe {
        let (st, slot) = stat_target(path)?;
        // Security fix (todo P1 #6): ownership/privilege check, mirroring
        // chmod. Boot-time callers (no current process) bypass, same as
        // chmod, so first-boot filesystem population keeps working.
        if !is_kernel_boot() {
            let cur = crate::proc::current();
            if !chown_allowed(cur.uid, cur.ring, st.uid) {
                return Err(Errno::Perm);
            }
        }
        with_mount(slot, || onyxfs::set_uid_gid(st.ino, uid, gid))
    }
}

/// # Safety
///
/// Caller contract: token must be a live fd token of the calling context.
/// Ownership check is performed inside (security fix, todo P1 #6).
pub unsafe fn fchown(token: FdToken, uid: u32, gid: u32) -> KResult<()> {
    // SAFETY: fd_check validates idx and epoch; the stat/set pair runs in
    // the fd's mount context under FS_LOCK. Errors from stat are swallowed
    // to keep the previous best-effort behavior of the boot path, but a
    // FAILED stat cannot grant permission: st stays default (uid 0 =
    // root-owned), so a non-root caller is still rejected by chown_allowed.
    unsafe {
        let idx = fd_check(token)?;
        let fd = fd_get(idx);
        if fd.fs != Fs::Onyx {
            return Err(Errno::NoSys);
        }
        // Security fix (todo P1 #6): same policy as chown, resolved via the
        // fd's inode number in the fd's mount context.
        let mnt = fd.mnt as usize;
        if !is_kernel_boot() {
            let owner = with_mount(mnt, || {
                let mut st = onyxfs::OnyfsStat::default();
                onyxfs::stat(fd.ino, &mut st)?;
                Ok(st.uid)
            })?;
            let cur = crate::proc::current();
            if !chown_allowed(cur.uid, cur.ring, owner) {
                return Err(Errno::Perm);
            }
        }
        with_mount(mnt, || onyxfs::set_uid_gid(fd.ino, uid, gid))
    }
}

#[cfg(test)]
mod tests;

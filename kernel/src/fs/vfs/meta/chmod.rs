use crate::fs::onyxfs;
use crate::fs::vfs::{FdToken, Fs, fd_check, fd_get, is_kernel_boot, stat_target, with_mount};
use onyx_core::errno::{Errno, KResult};

/// # Safety
///
/// Caller contract: path comes from the syscall layer's parse_user_path
/// (kernel-side NUL-free slice); ownership check is performed inside.
/// stat_target/with_mount serialize access and activate the target mount's
/// context via FS_LOCK (supersedes the old "not serialized" note).
pub unsafe fn chmod(path: &[u8], mode: u32) -> KResult<()> {
    // SAFETY: stat_target validates the absolute path and rejects non-Onyx
    // filesystems with ENOSYS; the ownership check reads only this hart's
    // current process; set_mode runs in the resolved mount's context.
    unsafe {
        let (st, slot) = stat_target(path)?;
        if !is_kernel_boot() {
            let cur = crate::proc::current();
            if cur.uid != 0 && cur.uid != st.uid {
                return Err(Errno::Perm);
            }
        }
        with_mount(slot, || onyxfs::set_mode(st.ino, mode))
    }
}

/// # Safety
///
/// Caller contract: token must be a live fd token of the calling context;
/// ownership check is performed inside. with_mount activates the fd's mount
/// context under FS_LOCK.
pub unsafe fn fchmod(token: FdToken, mode: u32) -> KResult<()> {
    // SAFETY: fd_check validates idx and epoch; the stat + set_mode pair
    // runs in the fd's mount context (best-effort ownership check mirrors
    // the historical behavior: a failed stat keeps uid 0, which only a
    // root caller can override).
    unsafe {
        let idx = fd_check(token)?;
        let fd = fd_get(idx);
        if fd.fs != Fs::Onyx {
            return Err(Errno::NoSys);
        }
        let mnt = fd.mnt as usize;
        if !is_kernel_boot() {
            let owner = with_mount(mnt, || {
                let mut st = onyxfs::OnyfsStat::default();
                let _ = onyxfs::stat(fd.ino, &mut st);
                st.uid
            });
            let cur = crate::proc::current();
            if cur.uid != 0 && cur.uid != owner {
                return Err(Errno::Perm);
            }
        }
        with_mount(mnt, || onyxfs::set_mode(fd.ino, mode))
    }
}

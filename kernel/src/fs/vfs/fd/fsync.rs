use crate::fs::onyxfs;
use crate::fs::vfs::{FdToken, Fs, fd_check, fd_get, with_mount};
use onyx_core::errno::{Errno, KResult};

/// # Safety
///
/// Caller contract: token must be a live fd token of the calling context.
/// with_mount now serializes fsync across harts via FS_LOCK (superseding
/// the old "do not call from two harts" note).
pub unsafe fn fsync(token: FdToken) -> KResult<()> {
    // SAFETY: fd_check validates idx and epoch; with_mount swaps the onyxfs
    // singleton into this fd's mount context under FS_LOCK.
    unsafe {
        let idx = fd_check(token)?;
        let fd = fd_get(idx);
        match fd.fs {
            Fs::Onyx => with_mount(fd.mnt as usize, || onyxfs::fsync(fd.ino)),
            _ => Err(Errno::NoSys),
        }
    }
}

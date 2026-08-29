use crate::fs::onyxfs;
use crate::fs::vfs::{FdToken, Fs, fd_check, fd_get};
use onyx_core::errno::{Errno, KResult};

/// # Safety
///
/// Caller contract: token must be a live fd token of the calling context.
pub unsafe fn fsync(token: FdToken) -> KResult<()> {
    // SAFETY: fd_check validates idx and epoch. onyxfs::fsync takes NO
    // cross-hart lock (the journal is write-ahead crash recovery, not a
    // mutex); two harts flushing concurrently rely on the write-through
    // onyxfs_write path being idempotent per fd — caller contract: do not
    // call from multiple harts on the same inode.
    unsafe {
        let idx = fd_check(token)?;
        let fd = fd_get(idx);
        match fd.fs {
            Fs::Onyx => onyxfs::fsync(fd.ino),
            _ => Err(Errno::NoSys),
        }
    }
}

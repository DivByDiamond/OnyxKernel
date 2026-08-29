use crate::fs::onyxfs;
use crate::fs::vfs::{FdToken, fd_check, fd_get};
use onyx_core::errno::KResult;

/// Truncate a file to zero length (legacy API, used by SYS_truncate).
///
/// # Safety
///
/// Caller contract: token must be a live fd token of the calling context.
pub unsafe fn truncate(token: FdToken) -> KResult<()> {
    // SAFETY: fd_check validates idx and epoch. onyxfs::truncate takes no
    // cross-hart lock (journal is crash recovery only); concurrent truncates
    // are not serialized — caller must not race across harts.
    unsafe {
        let idx = fd_check(token)?;
        let fd = fd_get(idx);
        onyxfs::truncate(fd.ino)
    }
}

/// Truncate a file to an explicit length (POSIX ftruncate(2)).
///
/// Delegates to OnyxFS `truncate_to_length(ino, length)`, which handles
/// all three cases:
///   - length == 0: free all data blocks
///   - length < current_size: free blocks past `length`
///   - length > current_size: allocate zero-filled blocks for the extended range
///
/// # Safety
///
/// Caller contract: token must be a live fd token of the calling context.
pub unsafe fn truncate_to_length(token: FdToken, length: u64) -> KResult<()> {
    // SAFETY: fd_check validates idx and epoch. onyxfs::truncate_to_length
    // takes no cross-hart lock (journal is crash recovery only); concurrent
    // resizes are not serialized — caller must not race across harts.
    unsafe {
        let idx = fd_check(token)?;
        let fd = fd_get(idx);
        onyxfs::truncate_to_length(fd.ino, length)
    }
}

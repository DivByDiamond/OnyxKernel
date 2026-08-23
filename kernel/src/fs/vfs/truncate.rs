use super::{FdToken, fd_check, fd_get};
use crate::fs::onyxfs;
use onyx_core::errno::KResult;

/// Truncate a file to zero length (legacy API, used by SYS_truncate).
pub unsafe fn truncate(token: FdToken) -> KResult<()> { unsafe {
    let idx = fd_check(token)?;
    let fd = fd_get(idx);
    onyxfs::truncate(fd.ino)
}}

/// Truncate a file to an explicit length (POSIX ftruncate(2)).
///
/// Delegates to OnyxFS `truncate_to_length(ino, length)`, which handles
/// all three cases:
///   - length == 0: free all data blocks
///   - length < current_size: free blocks past `length`
///   - length > current_size: allocate zero-filled blocks for the extended range
pub unsafe fn truncate_to_length(token: FdToken, length: u64) -> KResult<()> { unsafe {
    let idx = fd_check(token)?;
    let fd = fd_get(idx);
    onyxfs::truncate_to_length(fd.ino, length)
}}

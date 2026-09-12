use crate::fs::onyxfs;
use crate::fs::vfs::{FdToken, Fs, fd_check, fd_get, with_mount};
use onyx_core::errno::{Errno, KResult};

/// Truncate a file to zero length (legacy API, used by SYS_truncate).
///
/// # Safety
///
/// Caller contract: token must be a live fd token of the calling context.
/// with_mount activates the fd's mount context under FS_LOCK (serialization
/// supersedes the old "caller must not race" note).
pub unsafe fn truncate(token: FdToken) -> KResult<()> {
    // SAFETY: fd_check validates idx and epoch; non-Onyx fds are rejected
    // before any singleton access (previously a Proc/Ipc fd's ino was
    // truncated on whichever Onyx volume owned the driver state).
    unsafe {
        let idx = fd_check(token)?;
        let fd = fd_get(idx);
        if fd.fs != Fs::Onyx {
            return Err(Errno::NoSys);
        }
        with_mount(fd.mnt as usize, || onyxfs::truncate(fd.ino))
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
/// with_mount activates the fd's mount context under FS_LOCK.
pub unsafe fn truncate_to_length(token: FdToken, length: u64) -> KResult<()> {
    // SAFETY: fd_check validates idx and epoch; the Onyx guard prevents
    // pseudo-fd inos from being resized on the Onyx volume (see truncate).
    unsafe {
        let idx = fd_check(token)?;
        let fd = fd_get(idx);
        if fd.fs != Fs::Onyx {
            return Err(Errno::NoSys);
        }
        with_mount(fd.mnt as usize, || {
            onyxfs::truncate_to_length(fd.ino, length)
        })
    }
}

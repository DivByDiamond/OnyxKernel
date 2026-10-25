use crate::fs::vfs::{FdToken, Fs, VfsFd, fd_check, fd_get, with_mount};
use crate::fs::{fat32, onyxfs};
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
        match fd.fs {
            Fs::Onyx => with_mount(fd.mnt as usize, || onyxfs::truncate(fd.ino)),
            Fs::Fat32 => {
                if fd.ino == 0 {
                    return Err(Errno::NoSys);
                }
                with_mount(fd.mnt as usize, || -> KResult<()> {
                    // Keep the first cluster: the dirent still points at it
                    // (the fd carries no name to clear the first-cluster
                    // field), and freeing a cluster the dirent references
                    // would let the next allocation hand it to another file.
                    fat32::truncate_chain(fd.ino, 1)?;
                    fat32::set_size(fd.dir, fd.ino, 0)
                })
            }
            _ => Err(Errno::NoSys),
        }
    }
}

/// Resize a FAT32 file to exactly `target` bytes: shrinking frees the tail of
/// the cluster chain, growing zero-fills the extension (clusters allocated by
/// `fat32::write` are not guaranteed to be clean after a shrink/grow cycle).
///
/// # Safety
///
/// `fd` must describe a live FAT32 file (parent cluster and first cluster,
/// both validated by the FAT32 layer) and the caller must already hold the
/// mount context via with_mount, which also serializes FAT32 I/O.
unsafe fn fat32_resize(fd: VfsFd, target: u32) -> KResult<()> {
    // SAFETY: fat32 writes are serialized by with_mount's FS_LOCK; the zero
    // buffer is a valid local and every chunk is clamped to it.
    unsafe {
        if target < fd.size {
            // Same first-cluster rule as truncate(): a zero-length file keeps
            // one allocated cluster so its dirent stays valid.
            fat32::truncate_chain(fd.ino, u64::from(target.max(1)))?;
        } else if target > fd.size {
            let zeros = [0u8; 512];
            let mut off = fd.size;
            while off < target {
                let chunk = (target - off).min(zeros.len() as u32);
                let n = fat32::write(fd.ino, zeros.as_ptr(), off, chunk)?;
                if n == 0 {
                    return Err(Errno::NoSpace);
                }
                off += n;
            }
        }
        fat32::set_size(fd.dir, fd.ino, target)
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
        match fd.fs {
            Fs::Onyx => with_mount(fd.mnt as usize, || {
                onyxfs::truncate_to_length(fd.ino, length)
            }),
            Fs::Fat32 => {
                if fd.ino == 0 {
                    return Err(Errno::NoSys);
                }
                let target = u32::try_from(length).map_err(|_| Errno::Range)?;
                with_mount(fd.mnt as usize, || fat32_resize(fd, target))
            }
            _ => Err(Errno::NoSys),
        }
    }
}

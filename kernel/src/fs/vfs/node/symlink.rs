use super::split_parent;
use crate::fs::onyxfs;
use crate::fs::vfs::{Fs, resolve_path, with_mount};
use onyx_core::errno::{Errno, KResult};
use onyx_core::formats::ONYFS_ROOT_INO;

/// Create a symbolic link at `linkpath` pointing to `target`.
///
/// Audit note (🟡 #3 + 🟡 #5): symlinks are only supported on OnyxFS —
/// procfs, devfs, ipcfs and fat32 paths return `Errno::NoSys`. This is
/// the correct POSIX return value for "operation not implemented on
/// this filesystem"; the previous code already returned NoSys but the
/// behavior was undocumented, which made it look like a stub bug. It
/// is now explicitly documented.
///
/// # Safety
///
/// Caller contract: target/linkpath come from the syscall layer's
/// parse_user_path (kernel-side slices). with_mount holds FS_LOCK and
/// activates the link's mount context for the whole operation.
pub unsafe fn symlink(target: &[u8], linkpath: &[u8]) -> KResult<()> {
    // SAFETY: both slices are kernel-side (from parse_user_path); the
    // singleton switch/run/restore is fenced by with_mount (FS_LOCK).
    unsafe {
        let (fs, rel, slot) = resolve_path(linkpath)?;
        if fs != Fs::Onyx {
            return Err(Errno::NoSys);
        }
        let (parent_rel, filename) = split_parent(rel);
        if filename.is_empty() {
            return Err(Errno::Inval);
        }
        with_mount(slot, || -> KResult<()> {
            let mut st = onyxfs::OnyfsStat::default();
            let parent_ino = if parent_rel.is_empty() {
                ONYFS_ROOT_INO
            } else {
                onyxfs::lookup(parent_rel, &mut st)?
            };
            onyxfs::symlink(parent_ino, filename, target)?;
            Ok(())
        })
    }
}

/// Read the target of a symbolic link at `path` into `buf`.
///
/// Audit note (🟡 #3): like `symlink`, `readlink` is only implemented
/// for OnyxFS. Other filesystems return `Errno::NoSys` (matching
/// POSIX's expected behavior when the operation is not supported).
///
/// # Safety
///
/// Caller contract: path comes from the syscall layer's parse_user_path
/// (kernel-side slice); buf is a validated, writable user range of bufsiz
/// bytes for user callers (checked upstream) or a valid kernel buffer.
/// with_mount activates the link's mount context under FS_LOCK.
pub unsafe fn readlink(path: &[u8], buf: *mut u8, bufsiz: u32) -> KResult<u32> {
    // SAFETY: path is kernel-side; buf validity is per the # Safety
    // contract and onyxfs::readlink bounds its copy to bufsiz.
    unsafe {
        let (fs, rel, slot) = resolve_path(path)?;
        if fs != Fs::Onyx {
            return Err(Errno::NoSys);
        }
        with_mount(slot, || -> KResult<u32> {
            let mut st = onyxfs::OnyfsStat::default();
            // readlink acts on the link itself — resolve the path without
            // following the final component (POSIX semantics).
            let ino = onyxfs::lookup_nofollow(rel, &mut st)?;
            onyxfs::readlink(ino, buf, bufsiz)
        })
    }
}

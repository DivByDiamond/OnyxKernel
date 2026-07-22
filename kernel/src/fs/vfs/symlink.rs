use super::{resolve_mount, Fs};
use crate::fs::onyxfs;
use onyx_core::errno::{Errno, KResult};
use onyx_core::formats::ONYFS_ROOT_INO;

unsafe fn split_parent(path: &[u8]) -> (&[u8], &[u8]) {
    let p = if !path.is_empty() && path[0] == b'/' {
        &path[1..]
    } else {
        path
    };
    match p.iter().rposition(|&b| b == b'/') {
        Some(idx) => (&p[..idx], &p[idx + 1..]),
        None => (&[], p),
    }
}

/// Create a symbolic link at `linkpath` pointing to `target`.
///
/// Audit note (🟡 #3 + 🟡 #5): symlinks are only supported on OnyxFS —
/// procfs, devfs, ipcfs and fat32 paths return `Errno::NoSys`. This is
/// the correct POSIX return value for "operation not implemented on
/// this filesystem"; the previous code already returned NoSys but the
/// behavior was undocumented, which made it look like a stub bug. It
/// is now explicitly documented.
pub unsafe fn symlink(target: &[u8], linkpath: &[u8]) -> KResult<()> {
    if linkpath.is_empty() || linkpath[0] != b'/' {
        return Err(Errno::Inval);
    }
    let name = &linkpath[1..];
    let (fs, _) = resolve_mount(name);
    if fs != Fs::Onyx {
        return Err(Errno::NoSys);
    }
    let (parent_path, filename) = split_parent(linkpath);
    if filename.is_empty() {
        return Err(Errno::Inval);
    }
    let mut st = onyxfs::OnyfsStat::default();
    let parent_ino = if parent_path.is_empty() {
        ONYFS_ROOT_INO
    } else {
        onyxfs::lookup(parent_path, &mut st)?
    };
    onyxfs::symlink(parent_ino, filename, target)?;
    Ok(())
}

/// Read the target of a symbolic link at `path` into `buf`.
///
/// Audit note (🟡 #3): like `symlink`, `readlink` is only implemented
/// for OnyxFS. Other filesystems return `Errno::NoSys` (matching
/// POSIX's expected behavior when the operation is not supported).
pub unsafe fn readlink(path: &[u8], buf: *mut u8, bufsiz: u32) -> KResult<u32> {
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
    onyxfs::readlink(ino, buf, bufsiz)
}

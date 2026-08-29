use crate::fs::onyxfs;
use crate::fs::vfs::{Fs, resolve_mount};
use onyx_core::errno::{Errno, KResult};
use onyx_core::formats::ONYFS_ROOT_INO;

/// # Safety
///
/// No unsafe operations inside; only bounds-checked slice arithmetic. The
/// unsafe signature mirrors the create.rs helper for symmetry.
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
///
/// # Safety
///
/// Caller contract: target/linkpath come from the syscall layer's
/// parse_user_path (kernel-side slices).
pub unsafe fn symlink(target: &[u8], linkpath: &[u8]) -> KResult<()> {
    // SAFETY: both slices are kernel-side (from parse_user_path). onyxfs::symlink
    // takes no cross-hart lock (journal is crash recovery only); concurrent
    // symlink creations are not serialized — caller must not race across harts.
    unsafe {
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
pub unsafe fn readlink(path: &[u8], buf: *mut u8, bufsiz: u32) -> KResult<u32> {
    // SAFETY: path is kernel-side; buf is a validated user range of bufsiz
    // bytes for user callers (user_ptr_ok upstream, translated) or a valid
    // kernel buffer; onyxfs::readlink bounds its copy to bufsiz.
    unsafe {
        if path.is_empty() || path[0] != b'/' {
            return Err(Errno::Inval);
        }
        let name = &path[1..];
        let (fs, _) = resolve_mount(name);
        if fs != Fs::Onyx {
            return Err(Errno::NoSys);
        }
        let mut st = onyxfs::OnyfsStat::default();
        // readlink acts on the link itself — resolve the path without following
        // the final component (POSIX semantics).
        let ino = onyxfs::lookup_nofollow(name, &mut st)?;
        onyxfs::readlink(ino, buf, bufsiz)
    }
}

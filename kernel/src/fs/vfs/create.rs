//! File creation — `create` (regular file) and `mkdir` (directory).
use super::{alloc_fd, fd_token, resolve_mount, FdToken, Fs, PERM_READ, PERM_SEEK, PERM_WRITE};
use crate::fs::onyxfs;
use crate::proc;
use onyx_core::errno::{Errno, KResult};

/// Split a NUL-free path like "/foo/bar/baz" into ("foo/bar", "baz").
/// The leading '/' is stripped. If the path has no '/', returns ("", "foo").
/// Used by `create` and `mkdir` to find the parent directory.
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

/// Create a new regular file at `path` and open it with read+write+seek
/// permissions. Returns the new fd token. `mode` is the OnyxFS mode bits
/// (e.g. `ONYFS_DT_REG`).
pub unsafe fn create(path: &[u8], mode: u32) -> KResult<FdToken> {
    if path.is_empty() || path[0] != b'/' {
        return Err(Errno::Inval);
    }
    // Reject creation under procfs.
    let name = &path[1..];
    let (fs, _) = super::resolve_mount(name);
    if fs == Fs::Proc {
        return Err(Errno::Perm);
    }
    let (parent_path, filename) = split_parent(path);
    if filename.is_empty() {
        return Err(Errno::Inval);
    }
    let mut st = onyxfs::OnyfsStat::default();
    let parent_ino = if parent_path.is_empty() {
        onyx_core::formats::ONYFS_ROOT_INO
    } else {
        onyxfs::lookup(parent_path, &mut st)?
    };
    let new_ino = onyxfs::create(parent_ino, filename, mode)?;
    let cur_uid = proc::current().uid;
    let cur_gid = proc::current().gid;
    let _ = onyxfs::set_uid_gid(new_ino, cur_uid, cur_gid);
    let idx = alloc_fd(PERM_READ | PERM_WRITE | PERM_SEEK)?;
    super::fd_set(idx, new_ino, 0, Fs::Onyx, 0);
    let fd = super::fd_get(idx);
    Ok(fd_token(idx, fd.epoch))
}

/// Create a new directory at `path`. Returns Ok(()) on success.
pub unsafe fn mkdir(path: &[u8]) -> KResult<()> {
    if path.is_empty() || path[0] != b'/' {
        return Err(Errno::Inval);
    }
    let name = &path[1..];
    let (fs, _) = resolve_mount(name);
    if fs == Fs::Proc {
        return Err(Errno::Perm);
    }
    let (parent_path, dirname) = split_parent(path);
    if dirname.is_empty() {
        return Err(Errno::Inval);
    }
    let mut st = onyxfs::OnyfsStat::default();
    let parent_ino = if parent_path.is_empty() {
        onyx_core::formats::ONYFS_ROOT_INO
    } else {
        onyxfs::lookup(parent_path, &mut st)?
    };
    let new_ino = onyxfs::mkdir(parent_ino, dirname)?;
    let cur_uid = proc::current().uid;
    let cur_gid = proc::current().gid;
    let _ = onyxfs::set_uid_gid(new_ino, cur_uid, cur_gid);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_parent_root() {
        unsafe {
            let (parent, name) = split_parent(b"/foo");
            assert_eq!(parent, b"");
            assert_eq!(name, b"foo");
        }
    }

    #[test]
    fn test_split_parent_nested() {
        unsafe {
            let (parent, name) = split_parent(b"/foo/bar/baz");
            assert_eq!(parent, b"foo/bar");
            assert_eq!(name, b"baz");
        }
    }

    #[test]
    fn test_split_parent_no_slash() {
        unsafe {
            let (parent, name) = split_parent(b"foo");
            assert_eq!(parent, b"");
            assert_eq!(name, b"foo");
        }
    }

    #[test]
    fn test_split_parent_trailing_slash() {
        unsafe {
            let (parent, name) = split_parent(b"/foo/bar/");
            assert_eq!(parent, b"foo/bar");
            assert_eq!(name, b"");
        }
    }

    #[test]
    fn test_split_parent_single_component() {
        unsafe {
            let (parent, name) = split_parent(b"/");
            assert_eq!(parent, b"");
            assert_eq!(name, b"");
        }
    }

    #[test]
    fn test_split_parent_deep_nested() {
        unsafe {
            let (parent, name) = split_parent(b"/a/b/c/d/e/f");
            assert_eq!(parent, b"a/b/c/d/e");
            assert_eq!(name, b"f");
        }
    }

    #[test]
    fn test_split_parent_empty() {
        unsafe {
            let (parent, name) = split_parent(b"");
            assert_eq!(parent, b"");
            assert_eq!(name, b"");
        }
    }
}

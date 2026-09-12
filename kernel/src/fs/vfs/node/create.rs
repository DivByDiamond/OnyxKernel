//! File creation — `create` (regular file) and `mkdir` (directory).
use super::split_parent;
use crate::fs::onyxfs;
use crate::fs::vfs::{
    FdToken, Fs, PERM_READ, PERM_SEEK, PERM_WRITE, alloc_fd, fd_set, fd_token, resolve_path,
    with_mount,
};
use crate::proc;
use onyx_core::errno::{Errno, KResult};
use onyx_core::formats::ONYFS_ROOT_INO;

/// Create a new regular file at `path` and open it with read+write+seek
/// permissions. Returns the new fd token. `mode` is the OnyxFS mode bits
/// (e.g. `ONYFS_DT_REG`). Creation is supported on OnyxFS mounts only;
/// FAT32 is a read-only driver here and pseudo-fs paths have no parent
/// directory concept in the VFS sense.
///
/// # Safety
///
/// Caller contract: path comes from the syscall layer's parse_user_path
/// (kernel-side slice); runs in the calling process's syscall context.
pub unsafe fn create(path: &[u8], mode: u32) -> KResult<FdToken> {
    // SAFETY: path is a kernel-side slice (from parse_user_path); the fd
    // slot written below was claimed by alloc_fd in this context.
    // with_mount holds FS_LOCK and swaps the onyxfs singleton into the
    // target mount's context for the duration of create + chown.
    unsafe {
        let (fs, rel, slot) = resolve_path(path)?;
        match fs {
            Fs::Proc => return Err(Errno::Perm),
            Fs::Onyx => {}
            _ => return Err(Errno::NoSys),
        }
        let (parent_rel, filename) = split_parent(rel);
        if filename.is_empty() {
            return Err(Errno::Inval);
        }
        // Read uid/gid before entering the fs critical section. During early
        // boot there may be no current process yet; kernel-created nodes then
        // belong to root rather than dereferencing a null current pointer.
        let (cur_uid, cur_gid) = proc::current_opt().map_or((0, 0), |p| (p.uid, p.gid));
        let new_ino = with_mount(slot, || -> KResult<u32> {
            let mut st = onyxfs::OnyfsStat::default();
            let parent_ino = if parent_rel.is_empty() {
                ONYFS_ROOT_INO
            } else {
                onyxfs::lookup(parent_rel, &mut st)?
            };
            let ino = onyxfs::create(parent_ino, filename, mode)?;
            let _ = onyxfs::set_uid_gid(ino, cur_uid, cur_gid);
            Ok(ino)
        })?;
        let idx = alloc_fd(PERM_READ | PERM_WRITE | PERM_SEEK)?;
        fd_set(idx, new_ino, 0, Fs::Onyx, 0, slot as u8);
        let fd = crate::fs::vfs::fd_get(idx);
        Ok(fd_token(idx, fd.epoch))
    }
}

/// Create a new directory at `path`. Returns Ok(()) on success.
///
/// # Safety
///
/// Caller contract: path comes from the syscall layer's parse_user_path
/// (kernel-side slice); runs in the calling process's syscall context.
pub unsafe fn mkdir(path: &[u8]) -> KResult<()> {
    // SAFETY: path is kernel-side. with_mount holds FS_LOCK around
    // mkdir + chown on the target mount's singleton context.
    unsafe {
        let (fs, rel, slot) = resolve_path(path)?;
        match fs {
            Fs::Proc => return Err(Errno::Perm),
            Fs::Onyx => {}
            _ => return Err(Errno::NoSys),
        }
        let (parent_rel, dirname) = split_parent(rel);
        if dirname.is_empty() {
            return Err(Errno::Inval);
        }
        let (cur_uid, cur_gid) = proc::current_opt().map_or((0, 0), |p| (p.uid, p.gid));
        with_mount(slot, || -> KResult<()> {
            let mut st = onyxfs::OnyfsStat::default();
            let parent_ino = if parent_rel.is_empty() {
                ONYFS_ROOT_INO
            } else {
                onyxfs::lookup(parent_rel, &mut st)?
            };
            let new_ino = onyxfs::mkdir(parent_ino, dirname)?;
            let _ = onyxfs::set_uid_gid(new_ino, cur_uid, cur_gid);
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_parent_root() {
        // SAFETY: split_parent performs only bounds-checked slice arithmetic.
        unsafe {
            let (parent, name) = split_parent(b"/foo");
            assert_eq!(parent, b"");
            assert_eq!(name, b"foo");
        }
    }

    #[test]
    fn test_split_parent_nested() {
        // SAFETY: split_parent performs only bounds-checked slice arithmetic.
        unsafe {
            let (parent, name) = split_parent(b"/foo/bar/baz");
            assert_eq!(parent, b"foo/bar");
            assert_eq!(name, b"baz");
        }
    }

    #[test]
    fn test_split_parent_no_slash() {
        // SAFETY: split_parent performs only bounds-checked slice arithmetic.
        unsafe {
            let (parent, name) = split_parent(b"foo");
            assert_eq!(parent, b"");
            assert_eq!(name, b"foo");
        }
    }

    #[test]
    fn test_split_parent_trailing_slash() {
        // SAFETY: split_parent performs only bounds-checked slice arithmetic.
        unsafe {
            let (parent, name) = split_parent(b"/foo/bar/");
            assert_eq!(parent, b"foo/bar");
            assert_eq!(name, b"");
        }
    }

    #[test]
    fn test_split_parent_single_component() {
        // SAFETY: split_parent performs only bounds-checked slice arithmetic.
        unsafe {
            let (parent, name) = split_parent(b"/");
            assert_eq!(parent, b"");
            assert_eq!(name, b"");
        }
    }

    #[test]
    fn test_split_parent_deep_nested() {
        // SAFETY: split_parent performs only bounds-checked slice arithmetic.
        unsafe {
            let (parent, name) = split_parent(b"/a/b/c/d/e/f");
            assert_eq!(parent, b"a/b/c/d/e");
            assert_eq!(name, b"f");
        }
    }

    #[test]
    fn test_split_parent_empty() {
        // SAFETY: split_parent performs only bounds-checked slice arithmetic.
        unsafe {
            let (parent, name) = split_parent(b"");
            assert_eq!(parent, b"");
            assert_eq!(name, b"");
        }
    }
}

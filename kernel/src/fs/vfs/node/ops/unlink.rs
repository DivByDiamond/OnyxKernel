use super::super::split_parent;
use crate::fs::vfs::{Fs, rel_to_abs, resolve_path, with_mount};
use crate::fs::{fat32, onyxfs};
use onyx_core::errno::{Errno, KResult};

/// # Safety
///
/// Caller contract: path comes from the syscall layer's parse_user_path
/// (kernel-side slice). with_mount holds FS_LOCK and swaps the target
/// mount's filesystem context in, so unlinks on different volumes serialize
/// instead of corrupting whichever mount owned the singletons. FAT32
/// unlinks resolve the parent directory first (dirents are found by name).
pub unsafe fn unlink(path: &[u8]) -> KResult<()> {
    // SAFETY: path is a kernel-side slice; buf is a fixed stack array
    // written by rel_to_abs before onyxfs::unlink reads it.
    unsafe {
        let (fs, rel, slot) = resolve_path(path)?;
        if fs == Fs::Fat32 {
            let (parent_rel, name) = split_parent(rel);
            if name.is_empty() {
                return Err(Errno::Inval);
            }
            return with_mount(slot, || {
                let parent = if parent_rel.is_empty() {
                    fat32::root_cluster()
                } else {
                    fat32::lookup_dir(parent_rel)?
                };
                fat32::unlink(parent, name)
            });
        }
        if fs != Fs::Onyx {
            return Err(Errno::NoSys);
        }
        if slot >= crate::fs::vfs::MAX_MOUNTS {
            // Root fs: the original absolute path is already onyxfs-shaped.
            with_mount(slot, || onyxfs::unlink(path))
        } else {
            let mut buf = [0u8; 256];
            let abs = rel_to_abs(rel, &mut buf).ok_or(Errno::Range)?;
            with_mount(slot, || onyxfs::unlink(abs))
        }
    }
}

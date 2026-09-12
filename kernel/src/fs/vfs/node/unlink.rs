use crate::fs::onyxfs;
use crate::fs::vfs::{Fs, rel_to_abs, resolve_path, with_mount};
use onyx_core::errno::{Errno, KResult};

/// # Safety
///
/// Caller contract: path comes from the syscall layer's parse_user_path
/// (kernel-side slice). with_mount holds FS_LOCK and swaps the onyxfs
/// singleton into the target mount's context, so unlinks on different
/// volumes serialize instead of corrupting whichever mount owned the
/// singletons.
pub unsafe fn unlink(path: &[u8]) -> KResult<()> {
    // SAFETY: path is a kernel-side slice; buf is a fixed stack array
    // written by rel_to_abs before onyxfs::unlink reads it.
    unsafe {
        let (fs, rel, slot) = resolve_path(path)?;
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

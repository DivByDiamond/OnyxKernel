use crate::fs::onyxfs;
use crate::fs::vfs::resolve_mount;
use onyx_core::errno::{Errno, KResult};

/// # Safety
///
/// Caller contract: path comes from the syscall layer's parse_user_path
/// (kernel-side slice). onyxfs::unlink takes no cross-hart lock (journal is
/// crash recovery only); concurrent unlinks are not serialized.
pub unsafe fn unlink(path: &[u8]) -> KResult<()> {
    // SAFETY: path is a kernel-side slice; mount check filters to OnyxFS
    // before delegating to onyxfs (which performs no cross-hart locking).
    unsafe {
        if path.is_empty() || path[0] != b'/' {
            return Err(Errno::Inval);
        }
        let name = &path[1..];
        let (fs, _) = resolve_mount(name);
        if fs != crate::fs::vfs::Fs::Onyx {
            return Err(Errno::NoSys);
        }
        onyxfs::unlink(path)
    }
}

use crate::fs::onyxfs;
use crate::fs::vfs::{Fs, resolve_mount};
use onyx_core::errno::{Errno, KResult};

/// # Safety
///
/// Caller contract: path comes from the syscall layer's parse_user_path
/// (kernel-side slice); timestamps are opaque u64 values.
pub unsafe fn utimens(path: &[u8], mtime: u64, atime: u64) -> KResult<()> {
    // SAFETY: path slice is kernel-side. onyxfs::set_timestamps takes no
    // cross-hart lock (journal is crash recovery only); concurrent utimens
    // calls are not serialized — caller must not race across harts.
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
        let ino = onyxfs::lookup(name, &mut st)?;
        onyxfs::set_timestamps(ino, mtime, atime)
    }
}

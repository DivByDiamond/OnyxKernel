use crate::fs::onyxfs;
use crate::fs::vfs::{stat_target, with_mount};
use onyx_core::errno::KResult;

/// # Safety
///
/// Caller contract: path comes from the syscall layer's parse_user_path
/// (kernel-side slice); timestamps are opaque u64 values. stat_target
/// validates the path and filters non-Onyx filesystems; with_mount runs the
/// update in the target mount's context under FS_LOCK.
pub unsafe fn utimens(path: &[u8], mtime: u64, atime: u64) -> KResult<()> {
    // SAFETY: see stat_target/with_mount contracts; path is kernel-side.
    unsafe {
        let (st, slot) = stat_target(path)?;
        with_mount(slot, || onyxfs::set_timestamps(st.ino, mtime, atime))
    }
}

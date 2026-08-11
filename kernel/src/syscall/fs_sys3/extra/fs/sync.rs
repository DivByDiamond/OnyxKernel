use onyx_core::errno::Errno;

use crate::fs::vfs;

#[inline(never)]
pub unsafe fn sys_fsync(fd: u64) -> i64 {
    match vfs::fsync(fd) {
        Ok(()) => 0,
        Err(e) => e.as_i64(),
    }
}

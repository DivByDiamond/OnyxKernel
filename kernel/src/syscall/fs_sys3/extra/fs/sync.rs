
use crate::fs::vfs;

#[inline(never)]
pub unsafe fn sys_fsync(fd: u64) -> i64 { unsafe {
    match vfs::fsync(fd) {
        Ok(()) => 0,
        Err(e) => e.as_i64(),
    }
}}

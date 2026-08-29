use crate::fs::vfs;

/// # Safety
///
/// Call only from the syscall path with a current process set; `fd` is
/// validated inside vfs and no user memory is touched.
#[inline(never)]
pub unsafe fn sys_fsync(fd: u64) -> i64 {
    // SAFETY: body performs no unsafe operations; the block only wraps the
    // vfs::fsync call for the unsafe-fn dispatch convention.
    unsafe {
        match vfs::fsync(fd) {
            Ok(()) => 0,
            Err(e) => e.as_i64(),
        }
    }
}

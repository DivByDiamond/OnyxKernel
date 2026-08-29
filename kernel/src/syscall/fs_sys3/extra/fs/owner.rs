use onyx_core::errno::Errno;

use crate::fs::vfs;
use crate::syscall::handler::parse_user_path;

/// # Safety
///
/// Call only from handler::handle's syscall path: current process set, ACL
/// checked; `path` is validated inside before use.
pub unsafe fn sys_chown(path: u64, uid: u32, gid: u32) -> i64 {
    // SAFETY: parse_user_path validates the user path internally and copies
    // it into a kernel stack buffer; only that kernel copy is used
    // afterwards.
    unsafe {
        let mut path_buf = [0u8; 256];
        let path_len = match parse_user_path(path, &mut path_buf) {
            Some(l) => l,
            None => return Errno::Inval.as_i64(),
        };
        let path_bytes = &path_buf[..path_len];
        match vfs::chown(path_bytes, uid, gid) {
            Ok(()) => 0,
            Err(e) => e.as_i64(),
        }
    }
}

/// # Safety
///
/// Call only from the syscall path with a current process set; `fd` is
/// validated inside vfs and no user memory is touched.
pub unsafe fn sys_fchown(fd: u64, uid: u32, gid: u32) -> i64 {
    // SAFETY: body performs no unsafe operations; the block only wraps the
    // vfs::fchown call for the unsafe-fn dispatch convention.
    unsafe {
        match vfs::fchown(fd, uid, gid) {
            Ok(()) => 0,
            Err(e) => e.as_i64(),
        }
    }
}

/// # Safety
///
/// Call only from handler::handle's syscall path: current process set, ACL
/// checked; `path` is validated inside before use.
pub unsafe fn sys_chmod(path: u64, mode: u64) -> i64 {
    // SAFETY: parse_user_path validates the user path internally and copies
    // it into a kernel stack buffer; only that kernel copy is used
    // afterwards.
    unsafe {
        let mut path_buf = [0u8; 256];
        let path_len = match parse_user_path(path, &mut path_buf) {
            Some(l) => l,
            None => return Errno::Inval.as_i64(),
        };
        let path_bytes = &path_buf[..path_len];
        match vfs::chmod(path_bytes, mode as u32) {
            Ok(()) => 0,
            Err(e) => e.as_i64(),
        }
    }
}

/// # Safety
///
/// Call only from the syscall path with a current process set; `fd` is
/// validated inside vfs and no user memory is touched.
pub unsafe fn sys_fchmod(fd: u64, mode: u64) -> i64 {
    // SAFETY: body performs no unsafe operations; the block only wraps the
    // vfs::fchmod call for the unsafe-fn dispatch convention.
    unsafe {
        match vfs::fchmod(fd, mode as u32) {
            Ok(()) => 0,
            Err(e) => e.as_i64(),
        }
    }
}

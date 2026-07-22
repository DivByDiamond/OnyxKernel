use onyx_core::errno::Errno;

use crate::fs::vfs;
use crate::syscall::handler::parse_user_path;

pub unsafe fn sys_chown(path: u64, uid: u32, gid: u32) -> i64 {
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

pub unsafe fn sys_fchown(fd: u64, uid: u32, gid: u32) -> i64 {
    match vfs::fchown(fd, uid, gid) {
        Ok(()) => 0,
        Err(e) => e.as_i64(),
    }
}

pub unsafe fn sys_chmod(path: u64, mode: u64) -> i64 {
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

pub unsafe fn sys_fchmod(fd: u64, mode: u64) -> i64 {
    match vfs::fchmod(fd, mode as u32) {
        Ok(()) => 0,
        Err(e) => e.as_i64(),
    }
}

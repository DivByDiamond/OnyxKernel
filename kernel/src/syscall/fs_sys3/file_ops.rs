use crate::fs::vfs;
use onyx_core::errno::Errno;

use super::super::handler::{parse_user_path, user_ptr_ok};

pub unsafe fn sys_dup(old_token: u64) -> i64 {
    match vfs::dup(old_token) {
        Ok(new_token) => new_token as i64,
        Err(e) => e.as_i64(),
    }
}

pub unsafe fn sys_pipe(pipefd: u64) -> i64 {
    if !user_ptr_ok(pipefd, 8) {
        return Errno::Inval.as_i64();
    }
    let (r_token, w_token) = match vfs::create_pipe() {
        Ok(t) => t,
        Err(e) => return e.as_i64(),
    };
    let out = pipefd as *mut u64;
    *out = r_token;
    *out.add(1) = w_token;
    0
}

pub unsafe fn sys_unlink(path: u64) -> i64 {
    let path_bytes = match parse_user_path(path) {
        Some(s) => s,
        None => return Errno::Inval.as_i64(),
    };
    match vfs::unlink(path_bytes) {
        Ok(()) => 0,
        Err(e) => e.as_i64(),
    }
}

pub unsafe fn sys_rename(old_path: u64, new_path: u64) -> i64 {
    let old = match parse_user_path(old_path) {
        Some(s) => s,
        None => return Errno::Inval.as_i64(),
    };
    let new = match parse_user_path(new_path) {
        Some(s) => s,
        None => return Errno::Inval.as_i64(),
    };
    match vfs::rename(old, new) {
        Ok(()) => 0,
        Err(e) => e.as_i64(),
    }
}

pub unsafe fn sys_truncate(path: u64) -> i64 {
    let path_bytes = match parse_user_path(path) {
        Some(s) => s,
        None => return Errno::Inval.as_i64(),
    };
    let token = match vfs::open(path_bytes, vfs::PERM_WRITE) {
        Ok(t) => t,
        Err(e) => return e.as_i64(),
    };
    let r = match vfs::truncate(token) {
        Ok(()) => 0,
        Err(e) => e.as_i64(),
    };
    vfs::close(token).ok();
    r
}

pub unsafe fn sys_access(path: u64, _mode: u64) -> i64 {
    let path_bytes = match parse_user_path(path) {
        Some(s) => s,
        None => return Errno::Inval.as_i64(),
    };
    let token = match vfs::open(path_bytes, vfs::PERM_READ) {
        Ok(t) => t,
        Err(e) => return e.as_i64(),
    };
    vfs::close(token).ok();
    0
}

pub unsafe fn sys_fcntl(fd: u64, cmd: u64, arg: u64) -> i64 {
    match cmd {
        0 => {
            match vfs::dup2(fd, arg) {
                Ok(new_fd) => new_fd as i64,
                Err(e) => e.as_i64(),
            }
        }
        _ => Errno::NoSys.as_i64(),
    }
}

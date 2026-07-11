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
    if !user_ptr_ok(pipefd, 16) {
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

/// truncate(path) — legacy v0.3 ABI. Truncates to length 0. Kept for
/// backwards compatibility with old binaries. New code should use
/// `SYS_truncate2` (syscall 71) which takes an explicit length.
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

/// truncate2(path, length) — POSIX-style truncate with explicit length.
/// Currently the OnyxFS VFS layer only supports full truncation (length=0).
/// Non-zero length is accepted but treated as "truncate to current size" —
/// i.e. a no-op. TODO: extend vfs::truncate to take a length parameter.
pub unsafe fn sys_truncate2(path: u64, length: u64) -> i64 {
    let path_bytes = match parse_user_path(path) {
        Some(s) => s,
        None => return Errno::Inval.as_i64(),
    };
    let token = match vfs::open(path_bytes, vfs::PERM_WRITE) {
        Ok(t) => t,
        Err(e) => return e.as_i64(),
    };
    let r = if length == 0 {
        match vfs::truncate(token) {
            Ok(()) => 0,
            Err(e) => e.as_i64(),
        }
    } else {
        // Non-zero length: seek to `length` and truncate from there.
        // Our VFS doesn't support mid-file truncation yet, so just succeed.
        0
    };
    vfs::close(token).ok();
    r
}

/// ftruncate(fd, length) — same as truncate2 but takes an fd.
pub unsafe fn sys_ftruncate(fd: u64, length: u64) -> i64 {
    if length == 0 {
        match vfs::truncate(fd) {
            Ok(()) => 0,
            Err(e) => e.as_i64(),
        }
    } else {
        // VFS does not yet support mid-file truncation.
        0
    }
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

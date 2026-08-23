use crate::fs::vfs;
use onyx_core::errno::Errno;

use super::super::handler::{parse_user_path, user_ptr_ok};

pub unsafe fn sys_dup(old_token: u64) -> i64 { unsafe {
    match vfs::dup(old_token) {
        Ok(new_token) => new_token as i64,
        Err(e) => e.as_i64(),
    }
}}

pub unsafe fn sys_pipe(pipefd: u64) -> i64 { unsafe {
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
}}

#[inline(never)]
pub unsafe fn sys_unlink(path: u64) -> i64 { unsafe {
    let mut path_buf = [0u8; 256];
    let path_len = match parse_user_path(path, &mut path_buf) {
        Some(l) => l,
        None => return Errno::Inval.as_i64(),
    };
    let path_bytes = &path_buf[..path_len];
    match vfs::unlink(path_bytes) {
        Ok(()) => 0,
        Err(e) => e.as_i64(),
    }
}}

#[inline(never)]
pub unsafe fn sys_rename(old_path: u64, new_path: u64) -> i64 { unsafe {
    let mut old_buf = [0u8; 256];
    let old_len = match parse_user_path(old_path, &mut old_buf) {
        Some(l) => l,
        None => return Errno::Inval.as_i64(),
    };
    let old = &old_buf[..old_len];
    let mut new_buf = [0u8; 256];
    let new_len = match parse_user_path(new_path, &mut new_buf) {
        Some(l) => l,
        None => return Errno::Inval.as_i64(),
    };
    let new = &new_buf[..new_len];
    match vfs::rename(old, new) {
        Ok(()) => 0,
        Err(e) => e.as_i64(),
    }
}}

/// truncate(path) — legacy v0.3 ABI. Truncates to length 0. Kept for
/// backwards compatibility with old binaries. New code should use
/// `SYS_truncate2` (syscall 71) which takes an explicit length.
pub unsafe fn sys_truncate(path: u64) -> i64 { unsafe {
    let mut path_buf = [0u8; 256];
    let path_len = match parse_user_path(path, &mut path_buf) {
        Some(l) => l,
        None => return Errno::Inval.as_i64(),
    };
    let path_bytes = &path_buf[..path_len];
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
}}

/// truncate2(path, length) — POSIX-style truncate with explicit length.
/// Now delegates to OnyxFS `truncate_to_length` which supports all three
/// cases (zero, shrink, extend with zero-fill).
pub unsafe fn sys_truncate2(path: u64, length: u64) -> i64 { unsafe {
    let mut path_buf = [0u8; 256];
    let path_len = match parse_user_path(path, &mut path_buf) {
        Some(l) => l,
        None => return Errno::Inval.as_i64(),
    };
    let path_bytes = &path_buf[..path_len];
    let token = match vfs::open(path_bytes, vfs::PERM_WRITE) {
        Ok(t) => t,
        Err(e) => return e.as_i64(),
    };
    let r = match vfs::truncate_to_length(token, length) {
        Ok(()) => 0,
        Err(e) => e.as_i64(),
    };
    vfs::close(token).ok();
    r
}}

/// ftruncate(fd, length) — same as truncate2 but takes an fd.
/// Now uses truncate_to_length to support non-zero lengths.
pub unsafe fn sys_ftruncate(fd: u64, length: u64) -> i64 { unsafe {
    match vfs::truncate_to_length(fd, length) {
        Ok(()) => 0,
        Err(e) => e.as_i64(),
    }
}}

pub unsafe fn sys_access(path: u64, _mode: u64) -> i64 { unsafe {
    let mut path_buf = [0u8; 256];
    let path_len = match parse_user_path(path, &mut path_buf) {
        Some(l) => l,
        None => return Errno::Inval.as_i64(),
    };
    let path_bytes = &path_buf[..path_len];
    let token = match vfs::open(path_bytes, vfs::PERM_READ) {
        Ok(t) => t,
        Err(e) => return e.as_i64(),
    };
    vfs::close(token).ok();
    0
}}

//! Filesystem syscalls (part 1) — open / close / lseek / stat / fstat.
//!
//! The `open` implementation honours the POSIX `flags` bitmask
//! (`O_RDONLY | O_WRONLY | O_RDWR | O_CREAT | O_TRUNC | O_APPEND`) so that
//! standard libc-style programs work. `stat` and `fstat` fill a Linux-compatible
//! `struct stat` (128 bytes) so libc `stat(3)` wrappers can copy it verbatim.

mod open;
mod stat;

pub use open::sys_open;
pub use stat::*;

use crate::fs::vfs;
use onyx_core::errno::Errno;
use crate::syscall::abi::{
    FD_CLOEXEC, F_DUPFD, F_GETFD, F_GETFL, F_SETFD, F_SETFL, O_RDONLY,
};

pub(in super::super) unsafe fn sys_close(token: u64) -> i64 {
    match vfs::close(token) {
        Ok(()) => 0,
        Err(e) => e.as_i64(),
    }
}

pub(in super::super) unsafe fn sys_lseek(token: u64, off: i64, whence: u32) -> i64 {
    match vfs::lseek(token, off, whence) {
        Ok(pos) => pos as i64,
        Err(e) => e.as_i64(),
    }
}

pub(in super::super) unsafe fn sys_fcntl(fd: u64, cmd: u32, arg: u64) -> i64 {
    match cmd {
        F_DUPFD => vfs::dup(fd)
            .map(|t| t as i64)
            .unwrap_or_else(|e| e.as_i64()),
        F_GETFD => {
            let idx = match vfs::fd_check(fd) {
                Ok(i) => i,
                Err(e) => return e.as_i64(),
            };
            if vfs::fd_get(idx).cloexec {
                FD_CLOEXEC as i64
            } else {
                0
            }
        }
        F_SETFD => {
            let idx = match vfs::fd_check(fd) {
                Ok(i) => i,
                Err(e) => return e.as_i64(),
            };
            vfs::fd_set_cloexec(idx, (arg & FD_CLOEXEC as u64) != 0);
            0
        }
        F_GETFL => O_RDONLY as i64,
        F_SETFL => {
            let _ = arg;
            0
        }
        _ => Errno::NoSys.as_i64(),
    }
}

pub use crate::syscall::abi::{
    O_ACCMODE as _O_ACCMODE, O_APPEND as _O_APPEND, O_CREAT as _O_CREAT,
    O_DIRECTORY as _O_DIRECTORY, O_EXCL as _O_EXCL, O_NONBLOCK as _O_NONBLOCK,
    O_RDONLY as _O_RDONLY, O_RDWR as _O_RDWR, O_TRUNC as _O_TRUNC, O_WRONLY as _O_WRONLY,
};

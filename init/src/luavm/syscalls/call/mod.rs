pub mod comm;
pub mod proc;
pub mod timer;

use super::consts::*;
use core::arch::asm;

#[inline]
pub unsafe fn write(fd: u64, buf: *const u8, len: usize) -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_WRITE, in("a0") fd, in("a1") buf as usize, in("a2") len, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn read(fd: u64, buf: *mut u8, len: u64) -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_READ, in("a0") fd, in("a1") buf as usize, in("a2") len, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn open(path: *const u8, flags: u64, mode: u64) -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_OPEN, in("a0") path as usize, in("a1") flags, in("a2") mode, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn close(fd: u64) -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_CLOSE, in("a0") fd, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn lseek(fd: u64, off: i64, whence: u32) -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_LSEEK, in("a0") fd, in("a1") off, in("a2") whence, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn stat(path: *const u8, st_buf: *mut u8) -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_STAT, in("a0") path as usize, in("a1") st_buf as usize, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn fstat(fd: u64, st_buf: *mut u8) -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_FSTAT, in("a0") fd, in("a1") st_buf as usize, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn readdir(dir: *const u8, name_out: *mut u8, len: u64) -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_READDIR, in("a0") dir as usize, in("a1") name_out as usize, in("a2") len, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn create(path: *const u8, mode: u64, _reserved: u64) -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_CREATE, in("a0") path as usize, in("a1") mode, in("a2") 0, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn write_fd(fd: u64, buf: *const u8, len: usize) -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_WRITE_FD, in("a0") fd, in("a1") buf as usize, in("a2") len, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn mkdir(path: *const u8) -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_MKDIR, in("a0") path as usize, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn dup(old_fd: u64) -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_DUP, in("a0") old_fd, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn pipe(pipefd: *mut u64) -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_PIPE, in("a0") pipefd as usize, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn unlink(path: *const u8) -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_UNLINK, in("a0") path as usize, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn rename(old_path: *const u8, new_path: *const u8) -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_RENAME, in("a0") old_path as usize, in("a1") new_path as usize, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn chdir(path: *const u8) -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_CHDIR, in("a0") path as usize, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn getcwd(buf: *mut u8, len: u64) -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_GETCWD, in("a0") buf as usize, in("a1") len, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn getdents64(fd: u64, buf: *mut u8, len: u64) -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_GETDENTS64, in("a0") fd, in("a1") buf as usize, in("a2") len, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn getdents(fd: u64, buf: *mut u8, len: u64) -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_GETDENTS, in("a0") fd, in("a1") buf as usize, in("a2") len, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn access(path: *const u8, mode: u64) -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_ACCESS, in("a0") path as usize, in("a1") mode, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn fcntl(fd: u64, cmd: u32, arg: u64) -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_FCNTL, in("a0") fd, in("a1") cmd, in("a2") arg, lateout("a0") ret);
    ret
}

mod fd;
use crate::syscalls::consts::*;
use core::arch::asm;
pub use fd::*;

/// # Safety
/// `path` is a readable NUL-terminated path.
#[inline]
pub unsafe fn open(path: *const u8, flags: u64, mode: u64) -> i64 {
    let ret;
    // SAFETY: The caller satisfies this syscall's documented ABI contract.
    unsafe {
        asm!("ecall", in("a7") SYS_OPEN, in("a0") path as usize, in("a1") flags, in("a2") mode, lateout("a0") ret);
    }
    ret
}
/// # Safety
/// `path` is NUL-terminated; `st_buf` is writable for the 128-byte stat record.
#[inline]
pub unsafe fn stat(path: *const u8, st_buf: *mut u8) -> i64 {
    let ret;
    // SAFETY: The caller satisfies this syscall's documented ABI contract.
    unsafe {
        asm!("ecall", in("a7") SYS_STAT, in("a0") path as usize, in("a1") st_buf as usize, lateout("a0") ret);
    }
    ret
}
/// # Safety
/// `dir` is NUL-terminated; `name_out` is writable for `len` bytes.
#[inline]
pub unsafe fn readdir(dir: *const u8, name_out: *mut u8, len: u64) -> i64 {
    let ret;
    // SAFETY: The caller satisfies this syscall's documented ABI contract.
    unsafe {
        asm!("ecall", in("a7") SYS_READDIR, in("a0") dir as usize, in("a1") name_out as usize, in("a2") len, lateout("a0") ret);
    }
    ret
}
/// # Safety
/// `path` is a readable NUL-terminated path.
#[inline]
pub unsafe fn create(path: *const u8, mode: u64, _reserved: u64) -> i64 {
    let ret;
    // SAFETY: The caller satisfies this syscall's documented ABI contract.
    unsafe {
        asm!("ecall", in("a7") SYS_CREATE, in("a0") path as usize, in("a1") mode, in("a2") 0, lateout("a0") ret);
    }
    ret
}
/// # Safety
/// `path` is a readable NUL-terminated path.
#[inline]
pub unsafe fn mkdir(path: *const u8) -> i64 {
    let ret;
    // SAFETY: The caller satisfies this syscall's documented ABI contract.
    unsafe {
        asm!("ecall", in("a7") SYS_MKDIR, in("a0") path as usize, lateout("a0") ret);
    }
    ret
}
/// # Safety
/// `path` is a readable NUL-terminated path.
#[inline]
pub unsafe fn unlink(path: *const u8) -> i64 {
    let ret;
    // SAFETY: The caller satisfies this syscall's documented ABI contract.
    unsafe {
        asm!("ecall", in("a7") SYS_UNLINK, in("a0") path as usize, lateout("a0") ret);
    }
    ret
}
/// # Safety
/// `old_path` and `new_path` are readable NUL-terminated paths.
#[inline]
pub unsafe fn rename(old_path: *const u8, new_path: *const u8) -> i64 {
    let ret;
    // SAFETY: The caller satisfies this syscall's documented ABI contract.
    unsafe {
        asm!("ecall", in("a7") SYS_RENAME, in("a0") old_path as usize, in("a1") new_path as usize, lateout("a0") ret);
    }
    ret
}
/// # Safety
/// `path` is a readable NUL-terminated path.
#[inline]
pub unsafe fn chdir(path: *const u8) -> i64 {
    let ret;
    // SAFETY: The caller satisfies this syscall's documented ABI contract.
    unsafe {
        asm!("ecall", in("a7") SYS_CHDIR, in("a0") path as usize, lateout("a0") ret);
    }
    ret
}
/// # Safety
/// `buf` is writable for `len` bytes.
#[inline]
pub unsafe fn getcwd(buf: *mut u8, len: u64) -> i64 {
    let ret;
    // SAFETY: The caller satisfies this syscall's documented ABI contract.
    unsafe {
        asm!("ecall", in("a7") SYS_GETCWD, in("a0") buf as usize, in("a1") len, lateout("a0") ret);
    }
    ret
}
/// # Safety
/// `path` is a readable NUL-terminated path.
#[inline]
pub unsafe fn access(path: *const u8, mode: u64) -> i64 {
    let ret;
    // SAFETY: The caller satisfies this syscall's documented ABI contract.
    unsafe {
        asm!("ecall", in("a7") SYS_ACCESS, in("a0") path as usize, in("a1") mode, lateout("a0") ret);
    }
    ret
}

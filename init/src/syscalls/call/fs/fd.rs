use crate::syscalls::consts::*;
use core::arch::asm;

/// # Safety
/// `buf` is readable for `len` bytes.
#[inline]
pub unsafe fn write(fd: u64, buf: *const u8, len: usize) -> i64 {
    let ret;
    // SAFETY: The caller satisfies this syscall's documented ABI contract.
    unsafe {
        asm!("ecall", in("a7") SYS_WRITE, in("a0") fd, in("a1") buf as usize, in("a2") len, lateout("a0") ret);
    }
    ret
}
/// # Safety
/// `buf` is writable for `len` bytes.
#[inline]
pub unsafe fn read(fd: u64, buf: *mut u8, len: u64) -> i64 {
    let ret;
    // SAFETY: The caller satisfies this syscall's documented ABI contract.
    unsafe {
        asm!("ecall", in("a7") SYS_READ, in("a0") fd, in("a1") buf as usize, in("a2") len, lateout("a0") ret);
    }
    ret
}
/// # Safety
/// Call only from an Onyx userspace process using the RISC-V syscall ABI.
#[inline]
pub unsafe fn close(fd: u64) -> i64 {
    let ret;
    // SAFETY: The caller satisfies this syscall's documented ABI contract.
    unsafe {
        asm!("ecall", in("a7") SYS_CLOSE, in("a0") fd, lateout("a0") ret);
    }
    ret
}
/// # Safety
/// Call only from an Onyx userspace process using the RISC-V syscall ABI.
#[inline]
pub unsafe fn lseek(fd: u64, off: i64, whence: u32) -> i64 {
    let ret;
    // SAFETY: The caller satisfies this syscall's documented ABI contract.
    unsafe {
        asm!("ecall", in("a7") SYS_LSEEK, in("a0") fd, in("a1") off, in("a2") whence, lateout("a0") ret);
    }
    ret
}
/// # Safety
/// `st_buf` is writable for the 128-byte ABI stat record.
#[inline]
pub unsafe fn fstat(fd: u64, st_buf: *mut u8) -> i64 {
    let ret;
    // SAFETY: The caller satisfies this syscall's documented ABI contract.
    unsafe {
        asm!("ecall", in("a7") SYS_FSTAT, in("a0") fd, in("a1") st_buf as usize, lateout("a0") ret);
    }
    ret
}
/// # Safety
/// `buf` is readable for `len` bytes.
#[inline]
pub unsafe fn write_fd(fd: u64, buf: *const u8, len: usize) -> i64 {
    let ret;
    // SAFETY: The caller satisfies this syscall's documented ABI contract.
    unsafe {
        asm!("ecall", in("a7") SYS_WRITE_FD, in("a0") fd, in("a1") buf as usize, in("a2") len, lateout("a0") ret);
    }
    ret
}
/// # Safety
/// Call only from an Onyx userspace process using the RISC-V syscall ABI.
#[inline]
pub unsafe fn dup(old_fd: u64) -> i64 {
    let ret;
    // SAFETY: The caller satisfies this syscall's documented ABI contract.
    unsafe {
        asm!("ecall", in("a7") SYS_DUP, in("a0") old_fd, lateout("a0") ret);
    }
    ret
}
/// # Safety
/// `pipefd` is writable for two u64 descriptors.
#[inline]
pub unsafe fn pipe(pipefd: *mut u64) -> i64 {
    let ret;
    // SAFETY: The caller satisfies this syscall's documented ABI contract.
    unsafe {
        asm!("ecall", in("a7") SYS_PIPE, in("a0") pipefd as usize, lateout("a0") ret);
    }
    ret
}
/// # Safety
/// `buf` is writable for `len` bytes.
#[inline]
pub unsafe fn getdents64(fd: u64, buf: *mut u8, len: u64) -> i64 {
    let ret;
    // SAFETY: The caller satisfies this syscall's documented ABI contract.
    unsafe {
        asm!("ecall", in("a7") SYS_GETDENTS64, in("a0") fd, in("a1") buf as usize, in("a2") len, lateout("a0") ret);
    }
    ret
}
/// # Safety
/// `buf` is writable for `len` bytes.
#[inline]
pub unsafe fn getdents(fd: u64, buf: *mut u8, len: u64) -> i64 {
    let ret;
    // SAFETY: The caller satisfies this syscall's documented ABI contract.
    unsafe {
        asm!("ecall", in("a7") SYS_GETDENTS, in("a0") fd, in("a1") buf as usize, in("a2") len, lateout("a0") ret);
    }
    ret
}
/// # Safety
/// Any pointer encoded in `arg` satisfies the selected command's ABI and lifetime.
#[inline]
pub unsafe fn fcntl(fd: u64, cmd: u32, arg: u64) -> i64 {
    let ret;
    // SAFETY: The caller satisfies this syscall's documented ABI contract.
    unsafe {
        asm!("ecall", in("a7") SYS_FCNTL, in("a0") fd, in("a1") cmd, in("a2") arg, lateout("a0") ret);
    }
    ret
}

/// # Safety
/// `fds` is writable for `nfds` ABI pollfd records (16 bytes each).
#[inline]
pub unsafe fn poll(fds: *mut u8, nfds: u64, timeout: i64) -> i64 {
    let ret;
    // SAFETY: The caller satisfies this syscall's documented ABI contract.
    unsafe {
        asm!("ecall", in("a7") SYS_POLL, in("a0") fds as usize, in("a1") nfds, in("a2") timeout as usize, lateout("a0") ret);
    }
    ret
}

/// # Safety
/// `event` is writable for the 8-byte ABI mouse record.
#[inline]
pub unsafe fn mouse_read(event: *mut u8) -> i64 {
    let ret;
    // SAFETY: The caller satisfies this syscall's documented ABI contract.
    unsafe {
        asm!("ecall", in("a7") SYS_MOUSE_READ, in("a0") event as usize, lateout("a0") ret);
    }
    ret
}

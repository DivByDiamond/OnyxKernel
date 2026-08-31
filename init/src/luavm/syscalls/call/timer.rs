use super::super::consts::*;
use core::arch::asm;

#[inline]
pub unsafe fn getuid() -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_GETUID, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn getgid() -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_GETGID, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn setuid(uid: u64) -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_SETUID, in("a0") uid, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn setgid(gid: u64) -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_SETGID, in("a0") gid, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn uname(buf: *mut u8) -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_UNAME, in("a0") buf as usize, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn gettimeofday(tv: *mut u64) -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_GETTIMEOFDAY, in("a0") tv as usize, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn nanosleep(req: *const u64, rem: *mut u64) -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_NANOSLEEP, in("a0") req as usize, in("a1") rem as usize, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn clock_gettime(clk_id: u64, ts: *mut u64) -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_CLOCK_GETTIME, in("a0") clk_id, in("a1") ts as usize, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn clock_getres(clk_id: u64, res: *mut u64) -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_CLOCK_GETRES, in("a0") clk_id, in("a1") res as usize, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn ioctl(fd: u64, request: u64, arg: u64) -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_IOCTL, in("a0") fd, in("a1") request, in("a2") arg, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn isatty(fd: u64) -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_ISATTY, in("a0") fd, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn getentropy(buf: *mut u8, len: u64) -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_GETENTROPY, in("a0") buf as usize, in("a1") len, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn brk(addr: u64) -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_BRK, in("a0") addr, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn sbrk(incr: i64) -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_SBRK, in("a0") incr, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn mmap(addr: u64, length: u64, prot: u64, flags: u64, fd: u64, offset: u64) -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_MMAP, in("a0") addr, in("a1") length, in("a2") prot, in("a3") flags, in("a4") fd, in("a5") offset, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn munmap(addr: u64, length: u64) -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_MUNMAP, in("a0") addr, in("a1") length, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn mprotect(addr: u64, len: u64, prot: u64) -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_MPROTECT, in("a0") addr, in("a1") len, in("a2") prot, lateout("a0") ret);
    ret
}

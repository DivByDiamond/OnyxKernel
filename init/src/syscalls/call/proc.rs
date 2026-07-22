use super::super::consts::*;
use core::arch::asm;

#[inline]
pub unsafe fn exit(code: u64) -> ! {
    asm!("ecall", in("a7") SYS_EXIT, in("a0") code);
    loop {}
}
#[inline]
pub fn yield_cpu() {
    unsafe {
        asm!("ecall", in("a7") SYS_YIELD);
    }
}
#[inline]
pub unsafe fn getpid() -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_GETPID, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn spawn(path: *const u8, argv: *const u64, ring_hint: u8) -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_SPAWN, in("a0") path as usize, in("a1") argv as usize, in("a2") ring_hint, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn wait(status_out: *mut i32) -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_WAIT, in("a0") status_out as usize, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn getring() -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_GETRING, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn dropping(target: u8) -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_DROPRING, in("a0") target, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn exec(path: *const u8, argv: *const u64) -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_EXEC, in("a0") path as usize, in("a1") argv as usize, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn execve(path: *const u8, argv: *const u64, envp: *const u64) -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_EXECVE, in("a0") path as usize, in("a1") argv as usize, in("a2") envp as usize, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn fork() -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_FORK, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn waitpid(pid: u64, status: *mut i32, options: u32) -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_WAITPID, in("a0") pid, in("a1") status as usize, in("a2") options, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn getppid() -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_GETPPID, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn setpgid(pid: u64, pgid: u64) -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_SETPGID, in("a0") pid, in("a1") pgid, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn setsid() -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_SETSID, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn getpgid(pid: u64) -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_GETPGID, in("a0") pid, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn kill(pid: u32, sig: u32) -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_KILL, in("a0") pid, in("a1") sig, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn sigmask(how: u32, sig: u32) -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_SIGMASK, in("a0") how, in("a1") sig, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn sigaction(signum: u32, act: *const u64, oldact: *mut u64) -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_SIGACTION, in("a0") signum, in("a1") act as usize, in("a2") oldact as usize, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn sigprocmask(how: u32, set: *const u64, oldset: *mut u64) -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_SIGPROCMASK, in("a0") how, in("a1") set as usize, in("a2") oldset as usize, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn sigreturn() -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_SIGRETURN, lateout("a0") ret);
    ret
}

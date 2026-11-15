use super::super::consts::*;
use core::arch::asm;

#[inline]
/// # Safety
/// Call only from an Onyx userspace process using the RISC-V syscall ABI.
pub unsafe fn exit(code: u64) -> ! {
    // SAFETY: The caller satisfies this syscall's documented ABI contract.
    unsafe {
        asm!("ecall", in("a7") SYS_EXIT, in("a0") code);
    }
    // intentional: busy-wait
    loop {
        core::hint::spin_loop();
    }
}
#[inline]
pub fn yield_cpu() {
    // SAFETY: Yield has no pointer arguments and only schedules the calling process.
    unsafe {
        asm!("ecall", in("a7") SYS_YIELD);
    }
}
#[inline]
/// # Safety
/// Call only from an Onyx userspace process using the RISC-V syscall ABI.
pub unsafe fn getpid() -> i64 {
    let ret;
    // SAFETY: The caller satisfies this syscall's documented ABI contract.
    unsafe {
        asm!("ecall", in("a7") SYS_GETPID, lateout("a0") ret);
    }
    ret
}
#[inline]
/// # Safety
/// `path` is NUL-terminated; `argv` is a terminated array of valid string pointers.
pub unsafe fn spawn(path: *const u8, argv: *const u64, ring_hint: u8) -> i64 {
    let ret;
    // SAFETY: The caller satisfies this syscall's documented ABI contract.
    unsafe {
        asm!("ecall", in("a7") SYS_SPAWN, in("a0") path as usize, in("a1") argv as usize, in("a2") ring_hint, lateout("a0") ret);
    }
    ret
}
#[inline]
/// # Safety
/// `status_out` is null or writable for one i32.
pub unsafe fn wait(status_out: *mut i32) -> i64 {
    let ret;
    // SAFETY: The caller satisfies this syscall's documented ABI contract.
    unsafe {
        asm!("ecall", in("a7") SYS_WAIT, in("a0") status_out as usize, lateout("a0") ret);
    }
    ret
}
#[inline]
/// # Safety
/// Call only from an Onyx userspace process using the RISC-V syscall ABI.
pub unsafe fn getring() -> i64 {
    let ret;
    // SAFETY: The caller satisfies this syscall's documented ABI contract.
    unsafe {
        asm!("ecall", in("a7") SYS_GETRING, lateout("a0") ret);
    }
    ret
}
#[inline]
/// # Safety
/// Call only from an Onyx userspace process using the RISC-V syscall ABI.
pub unsafe fn dropping(target: u8) -> i64 {
    let ret;
    // SAFETY: The caller satisfies this syscall's documented ABI contract.
    unsafe {
        asm!("ecall", in("a7") SYS_DROPRING, in("a0") target, lateout("a0") ret);
    }
    ret
}
#[inline]
/// # Safety
/// `path` is NUL-terminated; `argv` is a terminated array of valid string pointers.
pub unsafe fn exec(path: *const u8, argv: *const u64) -> i64 {
    let ret;
    // SAFETY: The caller satisfies this syscall's documented ABI contract.
    unsafe {
        asm!("ecall", in("a7") SYS_EXEC, in("a0") path as usize, in("a1") argv as usize, lateout("a0") ret);
    }
    ret
}
#[inline]
/// # Safety
/// `path`, `argv`, and `envp` contain valid, terminated strings and pointer arrays.
pub unsafe fn execve(path: *const u8, argv: *const u64, envp: *const u64) -> i64 {
    let ret;
    // SAFETY: The caller satisfies this syscall's documented ABI contract.
    unsafe {
        asm!("ecall", in("a7") SYS_EXECVE, in("a0") path as usize, in("a1") argv as usize, in("a2") envp as usize, lateout("a0") ret);
    }
    ret
}
#[inline]
/// # Safety
/// The process is single-threaded and its state can safely be copied into a child.
pub unsafe fn fork() -> i64 {
    let ret;
    // SAFETY: The caller satisfies this syscall's documented ABI contract.
    unsafe {
        asm!("ecall", in("a7") SYS_FORK, lateout("a0") ret);
    }
    ret
}
#[inline]
/// # Safety
/// `status` is null or writable for one i32.
pub unsafe fn waitpid(pid: u64, status: *mut i32, options: u32) -> i64 {
    let ret;
    // SAFETY: The caller satisfies this syscall's documented ABI contract.
    unsafe {
        asm!("ecall", in("a7") SYS_WAITPID, in("a0") pid, in("a1") status as usize, in("a2") options, lateout("a0") ret);
    }
    ret
}
#[inline]
/// # Safety
/// Call only from an Onyx userspace process using the RISC-V syscall ABI.
pub unsafe fn getppid() -> i64 {
    let ret;
    // SAFETY: The caller satisfies this syscall's documented ABI contract.
    unsafe {
        asm!("ecall", in("a7") SYS_GETPPID, lateout("a0") ret);
    }
    ret
}
#[inline]
/// # Safety
/// Call only from an Onyx userspace process using the RISC-V syscall ABI.
pub unsafe fn setpgid(pid: u64, pgid: u64) -> i64 {
    let ret;
    // SAFETY: The caller satisfies this syscall's documented ABI contract.
    unsafe {
        asm!("ecall", in("a7") SYS_SETPGID, in("a0") pid, in("a1") pgid, lateout("a0") ret);
    }
    ret
}
#[inline]
/// # Safety
/// Call only from an Onyx userspace process using the RISC-V syscall ABI.
pub unsafe fn setsid() -> i64 {
    let ret;
    // SAFETY: The caller satisfies this syscall's documented ABI contract.
    unsafe {
        asm!("ecall", in("a7") SYS_SETSID, lateout("a0") ret);
    }
    ret
}
#[inline]
/// # Safety
/// Call only from an Onyx userspace process using the RISC-V syscall ABI.
pub unsafe fn getpgid(pid: u64) -> i64 {
    let ret;
    // SAFETY: The caller satisfies this syscall's documented ABI contract.
    unsafe {
        asm!("ecall", in("a7") SYS_GETPGID, in("a0") pid, lateout("a0") ret);
    }
    ret
}
#[inline]
/// # Safety
/// Call only from an Onyx userspace process using the RISC-V syscall ABI.
pub unsafe fn kill(pid: u32, sig: u32) -> i64 {
    let ret;
    // SAFETY: The caller satisfies this syscall's documented ABI contract.
    unsafe {
        asm!("ecall", in("a7") SYS_KILL, in("a0") pid, in("a1") sig, lateout("a0") ret);
    }
    ret
}
#[inline]
/// # Safety
/// Call only from an Onyx userspace process using the RISC-V syscall ABI.
pub unsafe fn sigmask(how: u32, sig: u32) -> i64 {
    let ret;
    // SAFETY: The caller satisfies this syscall's documented ABI contract.
    unsafe {
        asm!("ecall", in("a7") SYS_SIGMASK, in("a0") how, in("a1") sig, lateout("a0") ret);
    }
    ret
}
#[inline]
/// # Safety
/// Non-null `act` and `oldact` point to readable/writable ABI sigaction records.
pub unsafe fn sigaction(signum: u32, act: *const u64, oldact: *mut u64) -> i64 {
    let ret;
    // SAFETY: The caller satisfies this syscall's documented ABI contract.
    unsafe {
        asm!("ecall", in("a7") SYS_SIGACTION, in("a0") signum, in("a1") act as usize, in("a2") oldact as usize, lateout("a0") ret);
    }
    ret
}
#[inline]
/// # Safety
/// Non-null `set` and `oldset` point to readable/writable signal masks.
pub unsafe fn sigprocmask(how: u32, set: *const u64, oldset: *mut u64) -> i64 {
    let ret;
    // SAFETY: The caller satisfies this syscall's documented ABI contract.
    unsafe {
        asm!("ecall", in("a7") SYS_SIGPROCMASK, in("a0") how, in("a1") set as usize, in("a2") oldset as usize, lateout("a0") ret);
    }
    ret
}
#[inline]
/// # Safety
/// Call only from the kernel-provided signal trampoline with its saved frame intact.
pub unsafe fn sigreturn() -> i64 {
    let ret;
    // SAFETY: The caller satisfies this syscall's documented ABI contract.
    unsafe {
        asm!("ecall", in("a7") SYS_SIGRETURN, lateout("a0") ret);
    }
    ret
}

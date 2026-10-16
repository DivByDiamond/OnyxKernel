//! Explicit bindings for the three ABI console descriptors.
use crate::fs::vfs::{fd_check, fd_get, fd_token};
use onyx_core::errno::{Errno, KResult};

/// Resolve a redirected standard descriptor in the current syscall context.
pub unsafe fn resolve_stdio(token: u64) -> u64 {
    unsafe {
        if token <= 2 {
            let fd = fd_get(token as usize);
            if fd.used { return fd_token(token as usize, fd.epoch); }
        }
        token
    }
}

/// Bind a backing descriptor to stdin/out/err. MAX restores the UART default.
/// Called only from the owning process's syscall, with preemption disabled.
pub unsafe fn bind_stdio(source: u64, target: u64) -> KResult<u64> {
    unsafe {
        if target > 2 { return Err(Errno::Inval); }
        if source == u64::MAX {
            crate::proc::current().fds[target as usize].used = false;
            return Ok(target);
        }
        let source = resolve_stdio(source);
        let idx = fd_check(source)?;
        let mut fd = fd_get(idx);
        let p = crate::proc::current();
        fd.epoch = p.fds[target as usize].epoch.wrapping_add(1).max(1);
        fd.cloexec = false;
        p.fds[target as usize] = fd;
        Ok(target)
    }
}

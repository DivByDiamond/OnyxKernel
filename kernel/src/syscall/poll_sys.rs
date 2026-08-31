//! poll(2) — non-blocking fd readiness multiplexer (todo P1 #1).
//!
//! The missing ABI piece for TUI programs with timers: a read() on stdin
//! blocks forever, so an htop-style loop needs poll() to wait on keyboard
//! input with a timeout. Console fds are probed via the UART LSR.DR peek
//! (uart::rx_ready); regular fds are "ready" when bytes remain before EOF
//! (a zero-size stream reports readable — its read returns EOF, which poll
//! counts as readable). Timeouts use uptime_us() with sched_yield waits.
//!
//! Onyx deviation from Linux: fds are 64-bit (idx, epoch) tokens, so
//! PollFd.fd is i64 and the struct is 16 bytes (see abi::PollFd). POLLERR
//! has no kernel producer yet (backends report errors inline on read/write).

use crate::arch::trap_frame::TrapFrame;
use crate::fs::vfs;
use crate::mm::vmm;
use crate::proc;
use crate::syscall::abi::{POLL_MAX_FDS, POLLIN, POLLNVAL, POLLOUT, PollFd};
use crate::syscall::handler::user_ptr_ok;
use onyx_core::errno::Errno;

/// Staging buffer capacity (bounded kernel stack usage per poll entry).
const STAGE_MAX: usize = POLL_MAX_FDS as usize;

/// Copy the pollfd array from user memory into the bounded staging buffer.
/// Returns Err(errno) on a bad pointer or an oversized nfds.
/// # Safety
///
/// Syscall path only: current process set; `fds`/`nfds` are untrusted user
/// values validated here before any access.
unsafe fn copyin_pollfds(fds: u64, nfds: u64) -> Result<([PollFd; STAGE_MAX], usize), i64> {
    // SAFETY: fds is validated by user_ptr_ok plus the per-page writable
    // check_user_range before copy_from_user touches it; nfds is capped at
    // STAGE_MAX so the staging buffer cannot overflow.
    unsafe {
        if nfds > POLL_MAX_FDS {
            return Err(Errno::Inval.as_i64());
        }
        let n = nfds as usize;
        if n == 0 {
            return Ok(([PollFd::zeroed(); STAGE_MAX], 0));
        }
        let bytes = n * core::mem::size_of::<PollFd>();
        if !user_ptr_ok(fds, bytes as u64)
            || vmm::check_user_range(proc::current().root_pa, fds, bytes as u64, true).is_err()
        {
            return Err(Errno::Fault.as_i64());
        }
        let mut out = [PollFd::zeroed(); STAGE_MAX];
        if vmm::copy_from_user(
            proc::current().root_pa,
            out.as_mut_ptr() as *mut u8,
            fds,
            bytes,
        )
        .is_err()
        {
            return Err(Errno::Fault.as_i64());
        }
        Ok((out, n))
    }
}

/// Copy `n` pollfds (with revents filled in) back to user memory.
/// # Safety
///
/// Syscall path only; `fds` must be the same (validated) pointer passed to
/// copyin_pollfds.
unsafe fn copyout_pollfds(fds: u64, src: &[PollFd], n: usize) -> i64 {
    // SAFETY: fds is re-validated exactly as in copyin_pollfds before
    // copy_to_user writes `bytes` from the kernel staging buffer.
    unsafe {
        let bytes = n * core::mem::size_of::<PollFd>();
        if !user_ptr_ok(fds, bytes as u64)
            || vmm::check_user_range(proc::current().root_pa, fds, bytes as u64, true).is_err()
        {
            return Errno::Fault.as_i64();
        }
        match vmm::copy_to_user(
            proc::current().root_pa,
            fds,
            src.as_ptr() as *const u8,
            bytes,
        ) {
            Ok(()) => 0,
            Err(_) => Errno::Fault.as_i64(),
        }
    }
}

/// Readiness for the virtual console fds (0/1/2) and invalid fd values.
fn revents_console(fd: i64, events: i32) -> i32 {
    match fd {
        0 => {
            let mut rv = 0;
            if events & POLLIN != 0 && crate::drivers::uart::rx_ready() {
                rv |= POLLIN;
            }
            rv
        }
        1 | 2 => {
            // Console output never blocks.
            if events & POLLOUT != 0 { POLLOUT } else { 0 }
        }
        _ => POLLNVAL,
    }
}

/// Readiness for a real (fd-table) fd. A bad token yields POLLNVAL; EOF
/// (pos == size) still reports POLLIN because read(2) completes
/// immediately — POSIX poll treats end-of-file as readable.
fn revents_token(token: u64, events: i32) -> i32 {
    // SAFETY: fd_check bounds-checks idx and epoch before any table access;
    // fd_get returns a plain copy, so no aliasing survives this call.
    unsafe {
        let idx = match vfs::fd_check(token) {
            Ok(i) => i,
            Err(_) => return POLLNVAL,
        };
        let f = vfs::fd_get(idx);
        let mut rv = 0;
        if events & POLLIN != 0 && (f.pos < f.size || f.size == 0) {
            rv |= POLLIN;
        }
        if events & POLLOUT != 0 && f.perms & vfs::PERM_WRITE != 0 {
            rv |= POLLOUT;
        }
        rv
    }
}

/// One readiness pass over the staging buffer. Returns the number of fds
/// with a nonzero revents (POLLNVAL counts, fd < 0 entries do not).
fn scan(pollfds: &mut [PollFd], n: usize) -> usize {
    let mut ready = 0usize;
    for p in &mut pollfds[..n] {
        p.revents = 0;
        if p.fd < 0 {
            // POSIX: negative fds are ignored entirely (events not required
            // to be zero; revents stays 0).
            continue;
        }
        p.revents = if p.fd <= 2 {
            revents_console(p.fd, p.events)
        } else {
            // revents_token revalidates the (idx, epoch) token internally.
            revents_token(p.fd as u64, p.events)
        };
        if p.revents != 0 {
            ready += 1;
        }
    }
    ready
}

/// SYS_poll: poll(fds, nfds, timeout_ms). Returns the number of ready fds,
/// 0 on timeout. timeout < 0 blocks indefinitely (yields between passes);
/// 0 is a single non-blocking pass; > 0 is a millisecond deadline.
/// # Safety
///
/// Call only from handler::handle's syscall path with this hart's live trap
/// frame; user pointers are validated inside.
pub unsafe fn sys_poll(tf: &mut TrapFrame, fds: u64, nfds: u64, timeout: i64) -> i64 {
    // SAFETY: the staging buffer is kernel stack memory; the only user
    // accesses are the validated copyin/copyout helpers and the readiness
    // helpers (kernel fd tables only); `tf` is live per the handler::handle
    // contract and sched_yield requires it while waiting.
    unsafe {
        let (mut pollfds, n) = match copyin_pollfds(fds, nfds) {
            Ok(v) => v,
            Err(e) => return e,
        };
        if n == 0 {
            // POSIX sleeps for `timeout` even with an empty set; without a
            // wake source a negative timeout would spin forever, so an
            // empty set is an immediate timeout (documented deviation).
            return 0;
        }
        let deadline = if timeout > 0 {
            crate::srv::timer::uptime_us().saturating_add(timeout as u64 * 1000)
        } else {
            0
        };
        loop {
            let ready = scan(&mut pollfds, n);
            if ready > 0 || timeout == 0 {
                copyout_pollfds(fds, &pollfds, n);
                return ready as i64;
            }
            if deadline != 0 && crate::srv::timer::uptime_us() >= deadline {
                // Timeout still publishes revents (e.g. POLLNVAL), like POSIX.
                copyout_pollfds(fds, &pollfds, n);
                return 0;
            }
            proc::sched_yield(tf);
        }
    }
}

//! Non-canonical termios byte-stream read (fd 0, ICANON off).
//!
//! Implements the four POSIX MIN/TIME cases (todo P1 #5). Extracted from
//! console_read.rs (250-line rule): this file owns the termios timing
//! matrix, console_read.rs owns mode dispatch, raw and cooked paths.

use crate::arch::trap_frame::TrapFrame;
use crate::proc;
use onyx_core::errno::Errno;

/// Next line-disciplined byte from the UART (Ctrl+C consumed by
/// filter_input). Shared with console_read via a small closure-free call.
#[inline]
fn next_byte() -> Option<u8> {
    crate::drivers::uart::getc().and_then(crate::syscall::tty::filter_input)
}

#[inline]
fn stdin_nonblock() -> bool {
    // SAFETY: proc::current reads this hart's current Proc; syscall context
    // guarantees one is set (same contract as every sys_* handler).
    unsafe { proc::current().stdin_flags & crate::syscall::abi::O_NONBLOCK != 0 }
}

/// VTIME in deciseconds -> microseconds (POSIX: TIME * 0.1s).
#[inline]
fn vtime_us(vtime: u8) -> u64 {
    vtime as u64 * 100_000
}

/// Non-canonical termios read implementing the four POSIX MIN/TIME cases
/// (todo P1 #5). `vmin`/`vtime` are the calling process's termios values:
/// - MIN=0, TIME=0: never block; return queued bytes now (0 if none).
/// - MIN=0, TIME>0: total timeout from read() entry; return on first byte
///   or deadline.
/// - MIN>0, TIME=0: block until MIN bytes (or the buffer) are full.
/// - MIN>0, TIME>0: block for the first byte, then return when MIN bytes
///   are in or TIME elapsed since the last byte (inter-byte timeout).
///
/// O_NONBLOCK overrides MIN: EAGAIN when no byte is queued, otherwise
/// return what is already available.
pub(in crate::syscall) fn noncanon_read(
    tf: &mut TrapFrame,
    dst: *mut u8,
    len: u64,
    vmin: u8,
    vtime: u8,
) -> i64 {
    let maxlen = len as usize;
    let want = (vmin as usize).min(maxlen);
    let now = crate::srv::timer::uptime_us();
    // MIN=0 & TIME>0: one total deadline for the whole read.
    let mut deadline = if vmin == 0 && vtime > 0 {
        now.saturating_add(vtime_us(vtime))
    } else {
        0
    };
    let mut n = 0usize;
    loop {
        match next_byte() {
            Some(b) => {
                // SAFETY: the n < maxlen guard below plus the console_read
                // contract (len writable user bytes at dst) bound this write.
                unsafe {
                    *dst.add(n) = b;
                }
                n += 1;
                if vmin > 0 && vtime > 0 {
                    // Inter-byte window: re-armed by every received byte.
                    deadline = crate::srv::timer::uptime_us().saturating_add(vtime_us(vtime));
                }
                if n >= want.max(1) || n >= maxlen {
                    break;
                }
            }
            None => {
                if vmin == 0 && vtime == 0 {
                    // Pure poll: deliver what is queued (possibly 0).
                    break;
                }
                let expired = deadline != 0
                    && crate::srv::timer::uptime_us() >= deadline
                    && (vmin > 0 || n == 0);
                if expired {
                    break;
                }
                if stdin_nonblock() {
                    if n == 0 {
                        return Errno::Again.as_i64();
                    }
                    break;
                }
                // SAFETY: syscall-path yield; tf is this hart's live trap
                // frame per the console_read caller contract.
                unsafe { proc::sched_yield(tf) };
            }
        }
        if n >= maxlen {
            break;
        }
    }
    n as i64
}

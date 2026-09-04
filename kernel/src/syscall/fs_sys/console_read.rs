//! Console (fd 0) line discipline for sys_read.
//!
//! Extracted from read_write.rs (todo P1 #3/#5). Owns the three read modes:
//! raw (TIOCSRAW), non-canonical termios with VMIN/VTIME (the MIN/TIME
//! matrix lives in noncanon_read.rs), and cooked line editing — plus
//! O_NONBLOCK handling for the virtual stdin fd. fds 0-2 are not in the fd
//! table, so O_NONBLOCK lives in Proc::stdin_flags.

use crate::arch::trap_frame::TrapFrame;
use crate::drivers::{fb, fb_term, uart};
use crate::proc;
use crate::syscall::abi::O_NONBLOCK;
use crate::syscall::tty::filter_input;
use onyx_core::errno::Errno;

use super::noncanon_read::noncanon_read;

/// Next line-disciplined byte from the UART, or None when the input FIFO is
/// empty. Ctrl+C is consumed here and turned into SIGINT (filter_input).
#[inline]
fn next_byte() -> Option<u8> {
    uart::getc().and_then(filter_input)
}

#[inline]
fn stdin_nonblock() -> bool {
    // SAFETY: proc::current reads this hart's current Proc; syscall context
    // guarantees one is set (same contract as every sys_* handler).
    unsafe { proc::current().stdin_flags & O_NONBLOCK != 0 }
}

#[inline]
fn echo_char(b: u8) {
    // Serialize against kinf!/kerr! and other harts' console writes
    // (crate::srv::klog::UART_LOCK) — same fix as sys_write
    // (fs_sys/read_write.rs): the UART MMIO register is shared hardware,
    // and this keystroke-echo path wrote to it completely unlocked,
    // another real cross-hart UART race found while chasing the SMP crash
    // (todo.md, "Отдельный SMP-краш под -smp 2").
    crate::srv::klog::UART_LOCK.lock();
    uart::putc(b);
    crate::srv::klog::UART_LOCK.unlock();
    if fb::enabled() {
        fb_term::ansi::console_putc(b);
    }
}

/// First byte of a read: honors O_NONBLOCK (None instead of blocking),
/// otherwise yields until a byte arrives.
fn first_byte(tf: &mut TrapFrame) -> Option<u8> {
    match next_byte() {
        Some(b) => Some(b),
        None => {
            if stdin_nonblock() {
                return None;
            }
            loop {
                if let Some(b) = next_byte() {
                    return Some(b);
                }
                // SAFETY: syscall-path yield; tf is this hart's live trap
                // frame per the console_read caller contract.
                unsafe { proc::sched_yield(tf) };
            }
        }
    }
}

/// # Safety
///
/// Call only from sys_read's fd-0 path: `buf` has already passed
/// user_buf_ok for `len` writable bytes, and `tf` is this hart's live trap
/// frame (sched_yield contract).
pub(in crate::syscall) unsafe fn console_read(tf: &mut TrapFrame, buf: u64, len: u64) -> i64 {
    // SAFETY: user_buf_ok in the caller verified every page of [buf,
    // buf+len) is a mapped writable user page; all writes below stay within
    // len bytes of dst (cooked mode writes at most len-1 chars plus a NUL).
    unsafe {
        if len == 0 {
            return 0;
        }
        let dst = buf as *mut u8;
        let p = proc::current();

        if p.raw_stdin {
            raw_read(tf, dst, len)
        } else if !p.term_icanon {
            noncanon_read(tf, dst, len, p.term_vmin, p.term_vtime)
        } else {
            cooked_read(tf, dst, len)
        }
    }
}

/// Raw mode (TIOCSRAW): no echo, no editing. Block for the first byte, then
/// drain everything already queued (ESC sequences must arrive in one read).
fn raw_read(tf: &mut TrapFrame, dst: *mut u8, len: u64) -> i64 {
    let first = match first_byte(tf) {
        Some(b) => b,
        None => return Errno::Again.as_i64(),
    };
    // SAFETY: the console_read contract guarantees len writable user bytes
    // at dst; this loop writes only dst[0..len).
    unsafe {
        *dst.add(0) = first;
        let mut n = 1usize;
        while n < len as usize {
            match next_byte() {
                Some(b) => {
                    *dst.add(n) = b;
                    n += 1;
                }
                None => break,
            }
        }
        n as i64
    }
}

/// Cooked mode: canonical line editing (echo, backspace, Enter -> \n).
/// Echo reflects the CALLING process's termios (TCSETS), not legacy globals.
fn cooked_read(tf: &mut TrapFrame, dst: *mut u8, len: u64) -> i64 {
    // O_NONBLOCK in canonical mode: POSIX would need line-buffer state to
    // know whether a complete line is pending. Best effort: EAGAIN only
    // when the hardware FIFO is completely empty; a partially typed line
    // still blocks for its completion (documented compromise, todo P1 #3).
    if stdin_nonblock() && !uart::rx_ready() {
        return Errno::Again.as_i64();
    }
    let echo = stdin_echo();
    let mut n: usize = 0;
    let max = (len - 1).max(1) as usize;
    loop {
        if n >= max {
            break;
        }
        match next_byte() {
            None => {
                if stdin_nonblock() && n == 0 && !uart::rx_ready() {
                    return Errno::Again.as_i64();
                }
                // SAFETY: syscall-path yield (see console_read contract).
                unsafe { proc::sched_yield(tf) };
            }
            Some(b) => {
                if b == b'\r' || b == b'\n' {
                    // SAFETY: n < max <= len-1, and dst has len writable
                    // user bytes per the console_read contract.
                    unsafe {
                        *dst.add(n) = b'\n';
                    }
                    if echo {
                        echo_char(b'\r');
                        echo_char(b'\n');
                    }
                    n += 1;
                    break;
                } else if b == 0x7F || b == 0x08 {
                    if n > 0 {
                        n -= 1;
                        if echo {
                            echo_char(0x08);
                            echo_char(b' ');
                            echo_char(0x08);
                        }
                    }
                } else {
                    // SAFETY: n < max <= len-1 per the loop guard above.
                    unsafe {
                        *dst.add(n) = b;
                    }
                    if echo {
                        echo_char(b);
                    }
                    n += 1;
                }
            }
        }
    }
    // SAFETY: n <= max; for len == 1 max is 1 and n == 1 skips this write,
    // so dst[n] stays within the len validated user bytes.
    unsafe {
        if (n as u64) < len {
            *dst.add(n) = 0;
        }
    }
    n as i64
}

#[inline]
fn stdin_echo() -> bool {
    // SAFETY: proc::current on the syscall path (see stdin_nonblock).
    unsafe { proc::current().term_echo }
}

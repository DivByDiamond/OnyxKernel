use crate::arch::trap_frame::TrapFrame;
use crate::drivers::uart;
use crate::fs::vfs;
use crate::mm::vmm;
use crate::proc;
use crate::syscall::tty::{ECHO_ENABLED, filter_input};
use onyx_core::errno::Errno;

use super::super::handler::user_ptr_ok;

/// Validate a user buffer for syscall access: range-check plus per-page
/// mapping/PTE_U verification so a bad pointer yields EFAULT instead of an
/// S-mode page fault (which halts the machine).
unsafe fn user_buf_ok(buf: u64, len: u64, write: bool) -> bool {
    unsafe {
        user_ptr_ok(buf, len)
            && vmm::check_user_range(proc::current().root_pa, buf, len, write).is_ok()
    }
}

pub(in super::super) unsafe fn sys_write(tf: &mut TrapFrame, fd: u64, buf: u64, len: u64) -> i64 {
    unsafe {
        if !user_buf_ok(buf, len, false) {
            return Errno::Fault.as_i64();
        }
        if fd == 1 || fd == 2 {
            let src = buf as *const u8;
            let mut written: i64 = 0;
            let mut i: u64 = 0;
            while i < len {
                let b = *src.add(i as usize);
                if b == b'\n' {
                    uart::putc(b'\r');
                }
                uart::putc(b);
                written += 1;
                i += 1;
            }
            let _ = tf;
            written
        } else {
            match vfs::write(fd, buf as *const u8, len as u32) {
                Ok(n) => n as i64,
                Err(e) => e.as_i64(),
            }
        }
    }
}

pub(in super::super) unsafe fn sys_read(tf: &mut TrapFrame, _fd: u64, buf: u64, len: u64) -> i64 {
    unsafe {
        if !user_buf_ok(buf, len, true) {
            return Errno::Fault.as_i64();
        }
        if _fd == 0 {
            if len == 0 {
                return 0;
            }
            let dst = buf as *mut u8;

            // ── Raw mode: return raw bytes, no echo, no editing ──────────
            // Used by the shell for tab completion and arrow-key history.
            // In raw mode we block until at least one byte is available, then
            // return as many bytes as are available (up to len). This lets
            // the shell read ESC sequences (3 bytes: ESC [ A) in one call.
            let p = proc::current();
            if p.raw_stdin {
                // Block for the first byte.
                let first = loop {
                    match uart::getc().and_then(filter_input) {
                        Some(b) => break b,
                        None => proc::sched_yield(tf),
                    }
                };
                *dst.add(0) = first;
                let mut n = 1usize;
                // Drain any additional bytes that are already in the UART
                // FIFO without blocking. This is essential for ESC sequences
                // (ESC [ A for Up arrow) — if we blocked after ESC, the
                // shell would never see the rest of the sequence.
                while n < len as usize {
                    match uart::getc().and_then(filter_input) {
                        Some(b) => {
                            *dst.add(n) = b;
                            n += 1;
                        }
                        None => break,
                    }
                }
                return n as i64;
            }

            // ── Cooked mode: line editing (echo, backspace, Enter) ───────
            let mut n: usize = 0;
            let max = (len - 1) as usize;
            while n < max {
                match uart::getc().and_then(filter_input) {
                    None => {
                        proc::sched_yield(tf);
                        continue;
                    }
                    Some(b) => {
                        if b == b'\r' || b == b'\n' {
                            *dst.add(n) = b'\n';
                            if ECHO_ENABLED {
                                uart::putc(b'\r');
                                uart::putc(b'\n');
                            }
                            n += 1;
                            break;
                        } else if b == 0x7F || b == 0x08 {
                            if n > 0 {
                                n -= 1;
                                if ECHO_ENABLED {
                                    uart::putc(0x08);
                                    uart::putc(b' ');
                                    uart::putc(0x08);
                                }
                            }
                        } else {
                            *dst.add(n) = b;
                            if ECHO_ENABLED {
                                uart::putc(b);
                            }
                            n += 1;
                        }
                    }
                }
            }
            *dst.add(n) = 0;
            n as i64
        } else if _fd <= 2 {
            Errno::BadFd.as_i64()
        } else {
            match vfs::read(_fd, buf as *mut u8, len as u32) {
                Ok(n) => n as i64,
                Err(e) => e.as_i64(),
            }
        }
    }
}

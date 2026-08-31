use crate::arch::trap_frame::TrapFrame;
use crate::drivers::uart;
use crate::fs::vfs;
use crate::mm::vmm;
use crate::proc;
use onyx_core::errno::Errno;

use super::super::handler::user_ptr_ok;
use super::console_read::console_read;

/// Validate a user buffer for syscall access: range-check plus per-page
/// mapping/PTE_U verification so a bad pointer yields EFAULT instead of an
/// S-mode page fault (which halts the machine).
/// # Safety
///
/// Caller must be on the syscall path with a live current-process root_pa;
/// `buf`/`len` are untrusted values validated here, not pre-verified.
unsafe fn user_buf_ok(buf: u64, len: u64, write: bool) -> bool {
    // SAFETY: only the two validators run here; user_ptr_ok is pure
    // arithmetic and check_user_range performs read-only PTE translation on
    // the current process's own root table (vmm translate contract).
    unsafe {
        user_ptr_ok(buf, len)
            && vmm::check_user_range(proc::current().root_pa, buf, len, write).is_ok()
    }
}

/// # Safety
///
/// Call only from handler::handle's syscall path: current process set, ACL
/// checked; `buf`/`len` are validated inside before any access.
pub(in super::super) unsafe fn sys_write(tf: &mut TrapFrame, fd: u64, buf: u64, len: u64) -> i64 {
    // SAFETY: user_buf_ok verified every page of [buf, buf+len) is a mapped
    // readable user page, so the byte reads on the uart path and the
    // vfs::write below only touch mapped user memory; `tf` is this hart's
    // live trap frame.
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
                // Framebuffer console: ANSI interpreter (colors, cursor,
                // erase, scroll regions). Skipped when no fb is present.
                if crate::drivers::fb::enabled() {
                    crate::drivers::fb_term::ansi::console_putc(b);
                }
                written += 1;
                i += 1;
            }
            if crate::drivers::fb::enabled() {
                crate::drivers::fb_term::ansi::console_cursor();
            }
            let _ = tf;
            written
        } else if let Some(rc) = crate::fs::pty::stream::write_hook(tf, fd, buf, len) {
            // PTY master/slave fds are byte streams; they bypass the
            // positional fd layer (see read_hook comment).
            rc
        } else {
            match vfs::write(fd, buf as *const u8, len as u32) {
                Ok(n) => n as i64,
                Err(e) => e.as_i64(),
            }
        }
    }
}

/// # Safety
///
/// Call only from handler::handle's syscall path: current process set, ACL
/// checked; `buf`/`len` are validated inside before any access.
pub(in super::super) unsafe fn sys_read(tf: &mut TrapFrame, _fd: u64, buf: u64, len: u64) -> i64 {
    // SAFETY: user_buf_ok verified every page of [buf, buf+len) is a mapped
    // writable user page; the fd-0 console path (console_read) and the vfs
    // backend below only write within len bytes of the validated buffer.
    unsafe {
        if !user_buf_ok(buf, len, true) {
            return Errno::Fault.as_i64();
        }
        if _fd == 0 {
            // Console line discipline (raw / non-canonical VMIN-VTIME /
            // cooked + O_NONBLOCK) lives in its own module (todo P1 #3/#5).
            console_read(tf, buf, len)
        } else if _fd <= 2 {
            Errno::BadFd.as_i64()
        } else if let Some(rc) = crate::fs::pty::stream::read_hook(tf, _fd, buf, len) {
            // PTY master/slave fds are byte streams, not position-based
            // files: their readiness lives in the pty rings, so they run
            // through this hook (blocking yield-loop / O_NONBLOCK here)
            // instead of the positional vfs::read path.
            rc
        } else {
            match vfs::read(_fd, buf as *mut u8, len as u32) {
                Ok(n) => n as i64,
                Err(e) => e.as_i64(),
            }
        }
    }
}

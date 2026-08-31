use core::ptr;
use onyx_core::errno::Errno;

use crate::drivers::uart;
use crate::fs::devfs;
use crate::fs::vfs;
use crate::mm::vmm;
use crate::proc;
use crate::syscall::abi::{TCGETS, TCSETS};
use crate::syscall::handler::user_ptr_ok;

const ECHO: u32 = 0o0000010;
const ICANON: u32 = 0o0000002;
const B9600: u32 = 0o0000015;

const C_CFLAG: usize = 8;
const C_LFLAG: usize = 12;
const C_CC: usize = 16;
const C_CC_VMIN: usize = 6;
const C_CC_VTIME: usize = 5;
const TERMIOS_SIZE: usize = 60;

/// Copyout of a fixed-size kernel buffer to user memory (EFAULT-safe).
/// # Safety
///
/// Caller must be on the syscall path with a live current-process root_pa;
/// `src` must be valid for reads of `len` bytes; `buf_va` is untrusted and
/// validated here.
unsafe fn put_user(buf_va: u64, src: *const u8, len: usize) -> i64 {
    // SAFETY: buf_va passed user_ptr_ok above and copy_to_user re-validates
    // each page as a writable user mapping before writing; `src` validity is
    // the caller's contract documented above.
    unsafe {
        if !user_ptr_ok(buf_va, len as u64) {
            return Errno::Fault.as_i64();
        }
        match vmm::copy_to_user(proc::current().root_pa, buf_va, src, len) {
            Ok(()) => 0,
            Err(e) => e.as_i64(),
        }
    }
}

/// Copyin of a fixed-size user buffer into kernel memory (EFAULT-safe).
/// # Safety
///
/// Caller must be on the syscall path with a live current-process root_pa;
/// `dst` must be valid for writes of `len` bytes; `buf_va` is untrusted and
/// validated here.
unsafe fn get_user(dst: *mut u8, buf_va: u64, len: usize) -> i64 {
    // SAFETY: buf_va passed user_ptr_ok above and copy_from_user re-validates
    // each page as a readable user mapping before reading; `dst` validity is
    // the caller's contract documented above.
    unsafe {
        if !user_ptr_ok(buf_va, len as u64) {
            return Errno::Fault.as_i64();
        }
        match vmm::copy_from_user(proc::current().root_pa, dst, buf_va, len) {
            Ok(()) => 0,
            Err(e) => e.as_i64(),
        }
    }
}

/// # Safety
///
/// Call only from handler::handle's syscall path: current process set, ACL
/// checked; `arg` is validated per request inside before any user write.
pub unsafe fn sys_ioctl(fd: u64, request: u64, arg: u64) -> i64 {
    // SAFETY: termios writes go into a 60-byte stack buffer read back via
    // put_user/get_user (which validate `arg` per page); the TIOCGWINSZ path
    // checks user_ptr_ok and resolves a live translation before the 8-byte
    // write; the devfs FB_INFO path pre-validates arg via check_user_range.
    unsafe {
        let token = fd;
        if let Ok(idx) = vfs::fd_check(token) {
            let f = vfs::fd_get(idx);
            if f.fs == vfs::Fs::Devfs {
                // FB_IOCTL_GET_INFO writes a 5-u32 struct through `arg` inside
                // devfs — validate the mapping here so a bad pointer yields
                // EFAULT instead of an S-mode fault.
                if f.ino == devfs::DEVFS_FB0_INO
                    && request == devfs::FB_IOCTL_GET_INFO
                    && vmm::check_user_range(proc::current().root_pa, arg, 20, true).is_err()
                {
                    return Errno::Fault.as_i64();
                }
                return match devfs::ioctl(f.ino, request, arg) {
                    Ok(v) => v,
                    Err(e) => e.as_i64(),
                };
            }
        }

        match request {
            TCGETS => {
                if arg == 0 {
                    return 0;
                }
                let mut buf = [0u8; TERMIOS_SIZE];

                // Report the CURRENT process's termios, not a global: each
                // process owns its line-discipline state.
                let p = proc::current();
                let mut lflag = 0u32;
                if p.term_echo {
                    lflag |= ECHO;
                }
                if p.term_icanon {
                    lflag |= ICANON;
                }

                let buf = buf.as_mut_ptr();
                ptr::write(buf.add(C_CFLAG) as *mut u32, B9600);
                ptr::write(buf.add(C_LFLAG) as *mut u32, lflag);
                ptr::write(buf.add(C_CC + C_CC_VMIN), p.term_vmin);
                ptr::write(buf.add(C_CC + C_CC_VTIME), p.term_vtime);
                put_user(arg, buf, TERMIOS_SIZE)
            }
            TCSETS => {
                if arg == 0 {
                    return 0;
                }
                let mut buf = [0u8; TERMIOS_SIZE];
                if get_user(buf.as_mut_ptr(), arg, TERMIOS_SIZE) != 0 {
                    return Errno::Fault.as_i64();
                }
                let lflag = ptr::read(buf.as_ptr().add(C_LFLAG) as *const u32);
                let cc_vmin = buf[C_CC + C_CC_VMIN];
                let cc_vtime = buf[C_CC + C_CC_VTIME];
                let p = proc::current();
                p.term_echo = (lflag & ECHO) != 0;
                p.term_icanon = (lflag & ICANON) != 0;
                p.term_vmin = cc_vmin;
                p.term_vtime = cc_vtime;
                0
            }
            0x5421 => {
                if fd != 0 {
                    return Errno::Inval.as_i64();
                }
                proc::current().raw_stdin = true;
                0
            }
            0x5422 => {
                if fd != 0 {
                    return Errno::Inval.as_i64();
                }
                proc::current().raw_stdin = false;
                0
            }
            0x5423 => {
                if proc::current().raw_stdin {
                    1
                } else {
                    0
                }
            }
            0x5413 => {
                // TIOCGWINSZ: real terminal size. On the framebuffer console
                // derive it from the fb geometry (ANSI cell grid 8x16); else
                // fall back to the UART default 80x24.
                if arg == 0 {
                    return 0;
                }
                if !user_ptr_ok(arg, 8) {
                    return Errno::Inval.as_i64();
                }
                let pa = crate::mm::vmm::translate(proc::current().root_pa, arg);
                if pa == 0 {
                    return Errno::Inval.as_i64();
                }
                let ws = pa as *mut u16;
                let (rows, cols) = if crate::drivers::fb::enabled() {
                    let cols = (crate::drivers::fb::width() / 8) as u16;
                    let rows = (crate::drivers::fb::height() / 16) as u16;
                    (rows.max(1), cols.max(1))
                } else {
                    (24, 80)
                };
                // ws_row, ws_col, ws_xpixel, ws_ypixel
                ptr::write(ws.add(0), rows);
                ptr::write(ws.add(1), cols);
                ptr::write(ws.add(2), 0);
                ptr::write(ws.add(3), 0);
                // SIGWINCH on the first size read after a resize (todo P2
                // #1): TUI programs that query winsize at startup still
                // observe geometry changes made before their handler was
                // installed.
                if crate::drivers::fb::resize::take_resized() {
                    let _ = proc::signal_foreground(proc::SIGWINCH);
                }
                0
            }
            0x541B => {
                // FIONREAD (todo P1 #2): real byte count immediately
                // readable without blocking. Console (fd 0): the NS16550
                // FIFO counter is not readable, LSR.DR is the hardware's
                // only occupancy signal, so report 1 (>=1 byte pending) or
                // 0. Regular fds: bytes left before EOF (size - pos).
                if arg == 0 {
                    return 0;
                }
                let count: u32 = if fd == 0 {
                    if uart::rx_ready() { 1 } else { 0 }
                } else if fd <= 2 {
                    0
                } else {
                    let idx = match vfs::fd_check(fd) {
                        Ok(i) => i,
                        Err(e) => return e.as_i64(),
                    };
                    let f = vfs::fd_get(idx);
                    f.size.saturating_sub(f.pos)
                };
                put_user(arg, core::ptr::addr_of!(count).cast::<u8>(), 4)
            }
            _ => Errno::NoSys.as_i64(),
        }
    }
}

/// # Safety
///
/// Call only from the syscall path; the body is a stub that ignores `fd`
/// and touches no memory.
pub unsafe fn sys_isatty(fd: u64) -> i64 {
    let _ = fd;
    1
}

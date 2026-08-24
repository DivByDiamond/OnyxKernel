use core::ptr;
use onyx_core::errno::Errno;

use crate::fs::devfs;
use crate::fs::vfs;
use crate::mm::vmm;
use crate::proc;
use crate::syscall::abi::{TCGETS, TCSETS};
use crate::syscall::handler::user_ptr_ok;
use crate::syscall::tty::{ECHO_ENABLED, ICANON_ENABLED};

const ECHO: u32 = 0o0000010;
const ICANON: u32 = 0o0000002;
const B9600: u32 = 0o0000015;

// TODO(dead-code): C_IFLAG — termios layout offset, not handled yet, keep for layout doc.
#[allow(dead_code)]
const C_IFLAG: usize = 0;
// TODO(dead-code): C_OFLAG — termios layout offset, not handled yet, keep for layout doc.
#[allow(dead_code)]
const C_OFLAG: usize = 4;
const C_CFLAG: usize = 8;
const C_LFLAG: usize = 12;
const C_CC: usize = 16;
const C_CC_VMIN: usize = 6;
const C_CC_VTIME: usize = 5;
const TERMIOS_SIZE: usize = 60;

pub unsafe fn sys_ioctl(fd: u64, request: u64, arg: u64) -> i64 {
    let token = fd;
    if let Ok(idx) = vfs::fd_check(token) {
        let f = vfs::fd_get(idx);
        if f.fs == vfs::Fs::Devfs {
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
            if !user_ptr_ok(arg, TERMIOS_SIZE as u64) {
                return Errno::Inval.as_i64();
            }
            let pa = crate::mm::vmm::translate(proc::current().root_pa, arg);
            if pa == 0 {
                return Errno::Inval.as_i64();
            }
            let buf = pa as *mut u8;
            ptr::write_bytes(buf, 0, TERMIOS_SIZE);

            let mut lflag = 0u32;
            if ECHO_ENABLED {
                lflag |= ECHO;
            }
            if ICANON_ENABLED {
                lflag |= ICANON;
            }

            ptr::write(buf.add(C_CFLAG) as *mut u32, B9600);
            ptr::write(buf.add(C_LFLAG) as *mut u32, lflag);
            ptr::write(buf.add(C_CC + C_CC_VMIN), 1u8);
            ptr::write(buf.add(C_CC + C_CC_VTIME), 0u8);
            0
        }
        TCSETS => {
            if arg == 0 {
                return 0;
            }
            if !user_ptr_ok(arg, TERMIOS_SIZE as u64) {
                return Errno::Inval.as_i64();
            }
            let pa = crate::mm::vmm::translate(proc::current().root_pa, arg);
            if pa == 0 {
                return Errno::Inval.as_i64();
            }
            let buf = pa as *const u8;
            let lflag = ptr::read(buf.add(C_LFLAG) as *const u32);
            ECHO_ENABLED = (lflag & ECHO) != 0;
            ICANON_ENABLED = (lflag & ICANON) != 0;
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
            *ws = rows;
            *ws.add(1) = cols;
            *ws.add(2) = 0;
            *ws.add(3) = 0;
            0
        }
        0x541B => {
            if arg == 0 {
                return 0;
            }
            if !user_ptr_ok(arg, 4) {
                return Errno::Inval.as_i64();
            }
            let pa = crate::mm::vmm::translate(proc::current().root_pa, arg);
            if pa == 0 {
                return Errno::Inval.as_i64();
            }
            *(pa as *mut u32) = 0;
            0
        }
        _ => Errno::NoSys.as_i64(),
    }
}

pub unsafe fn sys_isatty(fd: u64) -> i64 {
    let _ = fd;
    1
}

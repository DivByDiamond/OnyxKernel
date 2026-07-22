use core::ptr;
use onyx_core::errno::Errno;

use crate::fs::devfs;
use crate::fs::vfs;
use crate::mm::vmm;
use crate::proc;
use crate::syscall::abi::{TCGETS, TCSETS};
use crate::syscall::handler::user_ptr_ok;

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
            if !user_ptr_ok(arg, 60) {
                return Errno::Inval.as_i64();
            }
            let pa = crate::mm::vmm::translate(proc::current().root_pa, arg);
            if pa == 0 {
                return Errno::Inval.as_i64();
            }
            core::ptr::write_bytes(pa as *mut u8, 0, 60);
            0
        }
        TCSETS => {
            let _ = (fd, arg);
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
            *ws = 24;
            *ws.add(1) = 80;
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

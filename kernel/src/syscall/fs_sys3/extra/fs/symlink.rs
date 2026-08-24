use onyx_core::errno::Errno;

use crate::fs::vfs;
use crate::proc;
use crate::syscall::handler::{parse_user_path, user_ptr_ok};

pub unsafe fn sys_readlink(path: u64, buf: u64, bufsiz: u64) -> i64 {
    unsafe {
        let mut path_buf = [0u8; 256];
        let path_len = match parse_user_path(path, &mut path_buf) {
            Some(l) => l,
            None => return Errno::Inval.as_i64(),
        };
        if buf == 0 || bufsiz == 0 || !user_ptr_ok(buf, bufsiz) {
            return Errno::Inval.as_i64();
        }
        // Verify every covered page is a mapped user page so vfs::readlink can
        // write through the user VA without risking an S-mode fault.
        if crate::mm::vmm::check_user_range(proc::current().root_pa, buf, bufsiz, true).is_err() {
            return Errno::Fault.as_i64();
        }
        let path_bytes = &path_buf[..path_len];
        match vfs::readlink(path_bytes, buf as *mut u8, bufsiz as u32) {
            Ok(n) => n as i64,
            Err(e) => e.as_i64(),
        }
    }
}

pub unsafe fn sys_symlink(target: u64, linkpath: u64) -> i64 {
    unsafe {
        let mut target_buf = [0u8; 256];
        let target_len = match parse_user_path(target, &mut target_buf) {
            Some(l) => l,
            None => return Errno::Inval.as_i64(),
        };
        let mut linkpath_buf = [0u8; 256];
        let linkpath_len = match parse_user_path(linkpath, &mut linkpath_buf) {
            Some(l) => l,
            None => return Errno::Inval.as_i64(),
        };
        let target_bytes = &target_buf[..target_len];
        let linkpath_bytes = &linkpath_buf[..linkpath_len];
        match vfs::symlink(target_bytes, linkpath_bytes) {
            Ok(()) => 0,
            Err(e) => e.as_i64(),
        }
    }
}

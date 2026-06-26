use crate::fs::onyxfs;
use crate::proc;

use super::super::handler::{parse_user_path, user_ptr_ok};

pub unsafe fn sys_chdir(path: u64) -> i64 {
    let path_bytes = match parse_user_path(path) {
        Some(s) => s,
        None => return onyx_core::errno::Errno::Inval.as_i64(),
    };
    match onyxfs::resolve_dir(path_bytes) {
        Ok(_ino) => {
            proc::set_cwd(path_bytes);
            0
        }
        Err(e) => e.as_i64(),
    }
}

pub unsafe fn sys_getcwd(buf: u64, len: u64) -> i64 {
    if !user_ptr_ok(buf, len) {
        return onyx_core::errno::Errno::Inval.as_i64();
    }
    let cwd = proc::cwd();
    let n = cwd.len().min(len as usize - 1);
    core::ptr::copy_nonoverlapping(cwd.as_ptr(), buf as *mut u8, n);
    *(buf as *mut u8).add(n) = 0;
    n as i64
}

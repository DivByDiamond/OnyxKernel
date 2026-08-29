use crate::fs::onyxfs;
use crate::proc;

use super::super::handler::{parse_user_path, user_ptr_ok};

#[inline(never)]
/// # Safety
///
/// Call only from handler::handle's syscall path: current process set, ACL
/// checked; `path` is validated inside before use.
pub unsafe fn sys_chdir(path: u64) -> i64 {
    // SAFETY: parse_user_path validates the user path internally and copies
    // it into a kernel stack buffer; set_cwd writes this hart's current
    // process cwd field with that kernel copy.
    unsafe {
        let mut path_buf = [0u8; 256];
        let path_len = match parse_user_path(path, &mut path_buf) {
            Some(l) => l,
            None => return onyx_core::errno::Errno::Inval.as_i64(),
        };
        let path_bytes = &path_buf[..path_len];
        match onyxfs::resolve_dir(path_bytes) {
            Ok(_ino) => {
                proc::set_cwd(path_bytes);
                0
            }
            Err(e) => e.as_i64(),
        }
    }
}

#[inline(never)]
/// # Safety
///
/// Call only from handler::handle's syscall path: current process set, ACL
/// checked; `buf`/`len` are validated inside before any write.
pub unsafe fn sys_getcwd(buf: u64, len: u64) -> i64 {
    // SAFETY: buf/len passed user_ptr_ok and the per-page writable
    // check_user_range above; both copy_to_user calls re-validate each page
    // before writing (cwd bytes from kernel memory, then the NUL).
    unsafe {
        if len == 0
            || !user_ptr_ok(buf, len)
            || crate::mm::vmm::check_user_range(proc::current().root_pa, buf, len, true).is_err()
        {
            return onyx_core::errno::Errno::Fault.as_i64();
        }
        let cwd = proc::cwd();
        let n = cwd.len().min(len as usize - 1);
        match crate::mm::vmm::copy_to_user(proc::current().root_pa, buf, cwd.as_ptr(), n).and_then(
            |()| {
                crate::mm::vmm::copy_to_user(
                    proc::current().root_pa,
                    buf + n as u64,
                    [0].as_ptr(),
                    1,
                )
            },
        ) {
            Ok(()) => n as i64,
            Err(e) => e.as_i64(),
        }
    }
}

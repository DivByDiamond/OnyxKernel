use super::handler::user_ptr_ok;
use crate::mm::vmm;
use crate::net;
use crate::proc;
use onyx_core::errno::Errno;

/// # Safety
///
/// Call only from the syscall path with a current process set; `ip_ptr` is
/// validated inside before use.
pub(super) unsafe fn sys_net_connect(ip_ptr: u64, port: u64) -> i64 {
    // SAFETY: the 4-byte ip range passed user_ptr_ok and per-page
    // check_user_range above; copy_from_user re-validates each page before
    // reading it into the stack buffer.
    unsafe {
        if !user_ptr_ok(ip_ptr, 4) || port == 0 || port > 65535 {
            return Errno::Inval.as_i64();
        }
        if vmm::check_user_range(proc::current().root_pa, ip_ptr, 4, false).is_err() {
            return Errno::Fault.as_i64();
        }
        let mut ip = [0u8; 4];
        if vmm::copy_from_user(proc::current().root_pa, ip.as_mut_ptr(), ip_ptr, 4).is_err() {
            return Errno::Fault.as_i64();
        }
        match net::tcp_connect(ip, port as u16) {
            Ok(cid) => cid as i64,
            Err(e) => e.as_i64(),
        }
    }
}

/// # Safety
///
/// Call only from the syscall path with a current process set; `buf`/`len`
/// are validated inside before use.
pub(super) unsafe fn sys_net_send(conn_id: u64, buf: u64, len: u64) -> i64 {
    // SAFETY: buf/len passed user_ptr_ok and the per-page readable
    // check_user_range above, so the slice handed to net::tcp_send covers
    // only mapped user pages.
    unsafe {
        if conn_id >= 8 || !user_ptr_ok(buf, len) {
            return Errno::Inval.as_i64();
        }
        // Verify every covered page is a mapped user page so the user-VA
        // slice below cannot trigger an S-mode page fault.
        if vmm::check_user_range(proc::current().root_pa, buf, len, false).is_err() {
            return Errno::Fault.as_i64();
        }
        let data = core::slice::from_raw_parts(buf as *const u8, len as usize);
        match net::tcp_send(conn_id as usize, data) {
            Ok(n) => n as i64,
            Err(e) => e.as_i64(),
        }
    }
}

/// # Safety
///
/// Call only from the syscall path with a current process set; `buf`/`len`
/// are validated inside before use.
pub(super) unsafe fn sys_net_recv(conn_id: u64, buf: u64, len: u64) -> i64 {
    // SAFETY: buf/len passed user_ptr_ok and the per-page writable
    // check_user_range above, so the slice handed to net::tcp_recv covers
    // only mapped, writable user pages.
    unsafe {
        if conn_id >= 8 || !user_ptr_ok(buf, len) {
            return Errno::Inval.as_i64();
        }
        if vmm::check_user_range(proc::current().root_pa, buf, len, true).is_err() {
            return Errno::Fault.as_i64();
        }
        let data = core::slice::from_raw_parts_mut(buf as *mut u8, len as usize);
        net::poll();
        match net::tcp_recv(conn_id as usize, data) {
            Ok(n) => n as i64,
            Err(e) => e.as_i64(),
        }
    }
}

/// # Safety
///
/// Call only from the syscall path; `conn_id` is bounds-checked inside and
/// no user memory is touched.
pub(super) unsafe fn sys_net_close(conn_id: u64) -> i64 {
    // SAFETY: body performs no unsafe operations; the block only wraps the
    // bounds check and net::tcp_close for the unsafe-fn dispatch convention.
    unsafe {
        if conn_id >= 8 {
            return Errno::Inval.as_i64();
        }
        net::tcp_close(conn_id as usize);
        0
    }
}

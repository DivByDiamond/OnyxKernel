//! Network syscalls — `sys_net_connect/send/recv/close` (TCP).
//!
//! Net sync fix (todo P1 #4): every `net::` entry point below takes the
//! recursive NET lock (`net::lock::net_lock`) internally, so no syscall-
//! level wrapping is needed here (and must NOT be added — it would only
//! duplicate the critical section and hold the lock across user-pointer
//! validation). The net layer is the single locking boundary.
use super::handler::{parse_user_path, user_ptr_ok};
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
/// Call only from the syscall path with a current process set; `name_ptr`
/// and `ip_out` are validated inside before use.
pub(super) unsafe fn sys_net_resolve(name_ptr: u64, ip_out: u64) -> i64 {
    // SAFETY: parse_user_path copies the hostname into a kernel stack
    // buffer; ip_out passed user_ptr_ok and the per-page writable
    // check_user_range below, so copy_to_user writes only mapped user
    // memory. dns_resolve only touches its own UDP socket and stack buffers.
    unsafe {
        let mut name_buf = [0u8; 256];
        let name_len = match parse_user_path(name_ptr, &mut name_buf) {
            Some(l) if l > 0 => l,
            _ => return Errno::Inval.as_i64(),
        };
        if !user_ptr_ok(ip_out, 4) {
            return Errno::Inval.as_i64();
        }
        if vmm::check_user_range(proc::current().root_pa, ip_out, 4, true).is_err() {
            return Errno::Fault.as_i64();
        }
        let dns_server = net::G_DNS;
        match net::dns_resolve(&name_buf[..name_len], dns_server) {
            Ok(ip) => match vmm::copy_to_user(proc::current().root_pa, ip_out, ip.as_ptr(), 4) {
                Ok(()) => 0,
                Err(_) => Errno::Fault.as_i64(),
            },
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

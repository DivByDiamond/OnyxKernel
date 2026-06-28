use super::handler::user_ptr_ok;
use crate::net;
use onyx_core::errno::Errno;

pub(super) unsafe fn sys_net_connect(ip_ptr: u64, port: u64) -> i64 {
    if !user_ptr_ok(ip_ptr, 4) || port == 0 || port > 65535 {
        return Errno::Inval.as_i64();
    }
    let ip = core::ptr::read_volatile(ip_ptr as *const [u8; 4]);
    match net::tcp_connect(ip, port as u16) {
        Ok(cid) => cid as i64,
        Err(e) => e.as_i64(),
    }
}

pub(super) unsafe fn sys_net_send(conn_id: u64, buf: u64, len: u64) -> i64 {
    if conn_id >= 8 || !user_ptr_ok(buf, len) {
        return Errno::Inval.as_i64();
    }
    let data = core::slice::from_raw_parts(buf as *const u8, len as usize);
    match net::tcp_send(conn_id as usize, data) {
        Ok(n) => n as i64,
        Err(e) => e.as_i64(),
    }
}

pub(super) unsafe fn sys_net_recv(conn_id: u64, buf: u64, len: u64) -> i64 {
    if conn_id >= 8 || !user_ptr_ok(buf, len) {
        return Errno::Inval.as_i64();
    }
    let data = core::slice::from_raw_parts_mut(buf as *mut u8, len as usize);
    net::poll();
    match net::tcp_recv(conn_id as usize, data) {
        Ok(n) => n as i64,
        Err(e) => e.as_i64(),
    }
}

pub(super) unsafe fn sys_net_close(conn_id: u64) -> i64 {
    if conn_id >= 8 {
        return Errno::Inval.as_i64();
    }
    net::tcp_close(conn_id as usize);
    0
}

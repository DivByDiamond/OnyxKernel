//! User-visible numeric socket IDs are process-owned capabilities. Check and
//! operate under one NET lock, excluding close/reallocation between both steps.
use super::lock::{net_lock, net_unlock};
use crate::net::{tcp, udp};
use onyx_core::errno::{Errno, KResult};
fn serialized<T>(operation: impl FnOnce() -> KResult<T>) -> KResult<T> {
    net_lock();
    let result = operation();
    net_unlock();
    result
}
pub fn tcp_connect(ip: [u8; 4], port: u16, pid: u32) -> KResult<usize> {
    serialized(|| unsafe {
        let id = tcp::tcp_connect(ip, port)?;
        tcp::conn::CONNS[id].as_mut().ok_or(Errno::Inval)?.owner_pid = pid;
        Ok(id)
    })
}
pub fn tcp<T>(id: usize, pid: u32, operation: impl FnOnce() -> KResult<T>) -> KResult<T> {
    serialized(|| unsafe {
        match tcp::conn::CONNS.get(id).and_then(Option::as_ref) {
            Some(socket) if socket.owner_pid == pid => operation(),
            Some(_) => Err(Errno::Perm),
            None => Err(Errno::Inval),
        }
    })
}
pub fn listen(port: u16, pid: u32) -> KResult<usize> {
    serialized(|| unsafe {
        let id = tcp::listen::listen(port)?;
        tcp::listen::set_owner(id, pid);
        Ok(id)
    })
}
pub fn listener<T>(id: usize, pid: u32, operation: impl FnOnce() -> KResult<T>) -> KResult<T> {
    serialized(|| unsafe {
        match tcp::listen::owner(id) {
            Some(owner) if owner == pid => operation(),
            Some(_) => Err(Errno::Perm),
            None => Err(Errno::Inval),
        }
    })
}
pub fn bind_udp(port: u16, pid: u32) -> KResult<usize> {
    serialized(|| unsafe {
        let id = udp::udp_bind_any(port)?;
        udp::UDP_SOCKS[id].as_mut().ok_or(Errno::Inval)?.owner_pid = pid;
        Ok(id)
    })
}
pub fn udp<T>(id: usize, pid: u32, operation: impl FnOnce() -> KResult<T>) -> KResult<T> {
    serialized(|| unsafe {
        match udp::UDP_SOCKS.get(id).and_then(Option::as_ref) {
            Some(socket) if socket.owner_pid == pid => operation(),
            Some(_) => Err(Errno::Perm),
            None => Err(Errno::Inval),
        }
    })
}
/// Called before process teardown takes process-list or runqueue locks.
/// Bounded table cleanup does no packet IO, allocation or scheduler work.
pub fn close_process(pid: u32) {
    net_lock();
    unsafe {
        for id in 0..8 {
            if tcp::listen::owner(id) == Some(pid) {
                let _ = tcp::listen::close(id);
            }
        }
        for slot in tcp::conn::CONNS.iter_mut() {
            if slot.as_ref().is_some_and(|s| s.owner_pid == pid) {
                if let Some(socket) = slot.as_mut().filter(|s| s.state == 3 || s.state == 4) {
                    socket.owner_pid = 0; // Kernel retains bounded FIN/data retries after close.
                } else {
                    *slot = None;
                }
            }
        }
        for slot in udp::UDP_SOCKS.iter_mut() {
            if slot.as_ref().is_some_and(|s| s.owner_pid == pid) {
                *slot = None;
            }
        }
    }
    net_unlock();
}
#[cfg(test)]
mod tests;

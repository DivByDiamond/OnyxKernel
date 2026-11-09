//! UDP slot allocation, connection setup, and teardown.

use crate::net::lock::{net_lock, net_unlock};
use onyx_core::errno::{Errno, KResult};

use super::super::{MAX_UDP_SOCKS, UDP_SOCKS, UdpSocket, alloc_udp_sock, next_udp_port};

/// # Safety
///
/// Allocates a slot in the lock-free UDP_SOCKS table; caller must
/// serialize bind/close across harts (no lock guards the table).
pub unsafe fn udp_bind(port: u16) -> KResult<usize> {
    // SAFETY: idx comes from alloc_udp_sock (a free slot), fully initialized
    // before publication; net_lock() excludes concurrent bind/close races
    // (net sync fix, todo P1 #4).
    unsafe {
        net_lock();
        if UDP_SOCKS
            .iter()
            .flatten()
            .any(|socket| socket.bound && socket.local_port == port)
        {
            net_unlock();
            return Err(Errno::Busy);
        }
        let idx = alloc_udp_sock().ok_or_else(|| {
            net_unlock();
            Errno::Busy
        })?;
        UDP_SOCKS[idx] = Some(UdpSocket {
            owner_pid: 0,
            iface: crate::drivers::virtio_net::active_device(),
            local_port: port,
            remote_ip: [0; 4],
            remote_port: 0,
            bound: true,
            connected: false,
            datagrams: super::super::datagram::DatagramQueue::EMPTY,
        });
        net_unlock();
        Ok(idx)
    }
}

/// # Safety
///
/// Allocates a slot in the lock-free UDP_SOCKS table; same
/// serialization contract as `udp_bind` (net_lock excludes concurrent
/// bind/close races).
///
/// Unlike `udp_bind` + `udp_sendto`, this binds an ephemeral local port
/// AND connects it to `(dst_ip, dst_port)` in one socket, so a later
/// `udp_send_bound` on the returned index sends *from* the same port
/// `udp_recv` listens on. `udp_sendto` sends from a throwaway temp
/// socket with its own ephemeral port that is closed immediately after
/// the send — a caller that also wants the reply (e.g. a request/response
/// protocol like DNS) would bind a second, unrelated port and never see
/// it, since replies are addressed to the port the request was sent from.
pub unsafe fn udp_bind_connect(dst_ip: [u8; 4], dst_port: u16) -> KResult<usize> {
    // SAFETY: idx comes from alloc_udp_sock (a free slot), fully
    // initialized before publication; net_lock() excludes concurrent
    // bind/close races (net sync fix, todo P1 #4).
    unsafe {
        net_lock();
        let idx = match alloc_udp_sock() {
            Some(i) => i,
            None => {
                net_unlock();
                return Err(Errno::Busy);
            }
        };
        let iface = match crate::net::config::route(dst_ip) {
            Some(iface) => iface,
            None => {
                net_unlock();
                return Err(Errno::Io);
            }
        };
        let port = next_udp_port();
        UDP_SOCKS[idx] = Some(UdpSocket {
            owner_pid: 0,
            iface,
            local_port: port,
            remote_ip: dst_ip,
            remote_port: dst_port,
            bound: true,
            connected: true,
            datagrams: super::super::datagram::DatagramQueue::EMPTY,
        });
        net_unlock();
        Ok(idx)
    }
}

/// # Safety
///
/// Drops slot `sock_idx`; caller must guarantee no other hart is
/// concurrently reading or writing that slot (no lock protects it).
pub unsafe fn udp_close(sock_idx: usize) {
    // SAFETY: plain slot overwrite under net_lock(); sock_idx is
    // bounds-checked so an out-of-range index cannot panic the kernel.
    unsafe {
        if sock_idx >= MAX_UDP_SOCKS {
            return;
        }
        net_lock();
        UDP_SOCKS[sock_idx] = None;
        net_unlock();
    }
}

/// # Safety
/// Privileged caller; binds a wildcard listener across all configured interfaces.
pub unsafe fn udp_bind_any(port: u16) -> KResult<usize> {
    // SAFETY: outer lock makes bind and wildcard marking atomic.
    unsafe {
        net_lock();
        let result = udp_bind(port);
        if let Ok(id) = result
            && let Some(socket) = UDP_SOCKS[id].as_mut()
        {
            socket.iface = usize::MAX;
        }
        net_unlock();
        result
    }
}

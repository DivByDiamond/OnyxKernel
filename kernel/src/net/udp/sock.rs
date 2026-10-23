use crate::net::G_IP;
use crate::net::checksum::pseudo_checksum;
use crate::net::ip;
use crate::net::lock::{net_lock, net_unlock};
use onyx_core::errno::{Errno, KResult};

use super::{
    MAX_UDP_SOCKS, UDP_HLEN, UDP_SOCKS, UdpSocket, alloc_udp_sock, next_udp_port,
};

/// # Safety
///
/// TX path: reads G_IP and the socket fields, drives the rings; the
/// caller owns `socket` exclusively (a &mut reference) and must not run
/// concurrent senders on other harts (net single-sender contract).
pub unsafe fn udp_send(socket: &mut UdpSocket, data: &[u8]) -> KResult<()> {
    // SAFETY: segment vector sized UDP_HLEN + data.len(); socket access is through the exclusive &mut above; net_lock() guards G_IP reads and the nested ip::send_packet (recursive, same hart).
    unsafe {
        net_lock();
        let result = send_endpoint(socket.local_port, socket.remote_ip, socket.remote_port, socket.connected, data);
        net_unlock();
        result
    }
}

/// Lock-free body of udp_send (caller holds net_lock; may already be nested).
///
/// # Safety
///
/// Caller contract: net_lock() held; `socket` exclusively owned by the caller.
unsafe fn send_endpoint(local_port: u16, remote_ip: [u8;4], remote_port: u16, connected: bool, data: &[u8]) -> KResult<()> {
    // SAFETY: segment vector sized UDP_HLEN + data.len(); socket access is through the exclusive &mut above.
    unsafe {
        if !connected || data.len() > 1472 {
            return Err(Errno::Inval);
        }
        let udp_len = UDP_HLEN + data.len();
        let mut seg = alloc::vec![0u8; udp_len];
        seg[0..2].copy_from_slice(&local_port.to_be_bytes());
        seg[2..4].copy_from_slice(&remote_port.to_be_bytes());
        seg[4..6].copy_from_slice(&(udp_len as u16).to_be_bytes());
        seg[6..8].copy_from_slice(&[0, 0]);
        if !data.is_empty() {
            seg[UDP_HLEN..].copy_from_slice(data);
        }
        let iface = crate::net::config::route(remote_ip).ok_or(Errno::Io)?;
        crate::net::config::with_interface(iface, || {
            let cksum = pseudo_checksum(&G_IP, &remote_ip, ip::IP_PROTO_UDP, &seg);
            let cksum=if cksum==0 {u16::MAX} else {cksum};
            seg[6..8].copy_from_slice(&cksum.to_be_bytes());
            ip::send_packet(remote_ip, ip::IP_PROTO_UDP, &seg)
        })
    }
}

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
        if UDP_SOCKS.iter().flatten().any(|socket|socket.bound && socket.local_port==port) { net_unlock();return Err(Errno::Busy); }
        let idx = alloc_udp_sock().ok_or_else(|| {
            net_unlock();
            Errno::Busy
        })?;
        UDP_SOCKS[idx] = Some(UdpSocket {
            owner_pid:0,
            iface: crate::drivers::virtio_net::active_device(),
            local_port: port,
            remote_ip: [0; 4],
            remote_port: 0,
            bound: true,
            connected: false,
            datagrams: super::datagram::DatagramQueue::EMPTY,
        });
        net_unlock();
        Ok(idx)
    }
}

/// # Safety
///
/// Temporarily occupies a UDP slot, sends and frees it; same
/// single-sender contract as the other socket ops -- the alloc->send
/// window is not lock-protected across harts.
pub unsafe fn udp_sendto(dst_ip: [u8; 4], dst_port: u16, data: &[u8]) -> KResult<()> {
    // SAFETY: temp slot fully initialized; alloc->use window is now atomic
    // w.r.t. other harts because the whole alloc->send->free sequence runs
    // under net_lock() (net sync fix, todo P1 #4).
    unsafe {
        net_lock();
        let result = udp_sendto_inner(dst_ip, dst_port, data);
        net_unlock();
        result
    }
}

/// Lock-free body of udp_sendto (caller holds net_lock).
///
/// # Safety
///
/// Caller contract: net_lock() held.
unsafe fn udp_sendto_inner(dst_ip: [u8; 4], dst_port: u16, data: &[u8]) -> KResult<()> {
    // SAFETY: temp slot fully initialized; alloc->use window re-checked fail-closed (see audit note below); net_lock() held by caller.
    unsafe {
        let idx = alloc_udp_sock().ok_or(Errno::Busy)?;
        let port = next_udp_port();
        UDP_SOCKS[idx] = Some(UdpSocket {
            owner_pid:0,
            iface: crate::net::config::route(dst_ip).ok_or(Errno::Io)?,
            local_port: port,
            remote_ip: dst_ip,
            remote_port: dst_port,
            bound: false,
            connected: true,
            datagrams: super::datagram::DatagramQueue::EMPTY,
        });
        let result = send_endpoint(port, dst_ip, dst_port, true, data);
        UDP_SOCKS[idx] = None;
        result
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
            None => { net_unlock(); return Err(Errno::Io); }
        };
        let port = next_udp_port();
        UDP_SOCKS[idx] = Some(UdpSocket {
            owner_pid:0,
            iface,
            local_port: port,
            remote_ip: dst_ip,
            remote_port: dst_port,
            bound: true,
            connected: true,
            datagrams: super::datagram::DatagramQueue::EMPTY,
        });
        net_unlock();
        Ok(idx)
    }
}

/// # Safety
///
/// Sends on a socket previously opened by `udp_bind_connect`; the caller
/// must own that slot exclusively (same single-sender contract as the
/// other socket ops).
pub unsafe fn udp_send_bound(sock_idx: usize, data: &[u8]) -> KResult<()> {
    // SAFETY: sock_idx bounds-checked below; socket access is through the
    // exclusive &mut from the table under net_lock() (recursive, same hart).
    unsafe {
        if sock_idx >= MAX_UDP_SOCKS {
            return Err(Errno::Inval);
        }
        net_lock();
        let endpoint = UDP_SOCKS[sock_idx].as_ref().map(|s| (s.local_port,s.remote_ip,s.remote_port,s.connected));
        let result = match endpoint {
            Some((local,remote,port,connected)) => send_endpoint(local,remote,port,connected,data),
            None => Err(Errno::Inval),
        };
        net_unlock();
        result
    }
}

/// # Safety
///
/// Reads/advances the receive ring of slot `sock_idx`; the caller must
/// own that slot exclusively -- a concurrent udp_recv or handle_udp on
/// the same slot from another hart would race the ring indices.
pub unsafe fn udp_recv(sock_idx: usize, buf: &mut [u8]) -> KResult<usize> {
    // SAFETY: delegated to the bounded, locked datagram receive operation.
    unsafe { udp_recv_from(sock_idx,buf).map(|(n,_)|n) }
}

/// # Safety
/// Kernel context; caller supplies a valid writable slice.
pub unsafe fn udp_recv_from(sock_idx: usize, buf: &mut [u8]) -> KResult<(usize,super::datagram::Peer)> {
    // SAFETY: bounds checked; socket table and queue are serialized by net_lock.
    unsafe {
        if sock_idx >= MAX_UDP_SOCKS { return Err(Errno::Inval); }
        net_lock();
        let result = match UDP_SOCKS[sock_idx].as_mut() {
            Some(socket) => socket.datagrams.pop(buf).ok_or(Errno::NoEnt),
            None => Err(Errno::Inval),
        };
        net_unlock();
        result
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
pub unsafe fn udp_bind_any(port:u16) -> KResult<usize> {
    // SAFETY: outer lock makes bind and wildcard marking atomic.
    unsafe { net_lock();let result=udp_bind(port);if let Ok(id)=result && let Some(socket)=UDP_SOCKS[id].as_mut() { socket.iface=usize::MAX; } net_unlock();result }
}

/// # Safety
/// Caller provides a valid payload; socket table and TX are serialized.
pub unsafe fn udp_send_from(id:usize, peer:super::datagram::Peer, data:&[u8]) -> KResult<()> {
    // SAFETY: bounds-checked socket lookup, endpoint copied before reentrant IO.
    unsafe {
        net_lock();
        let local=UDP_SOCKS.get(id).and_then(Option::as_ref).map(|socket|socket.local_port);
        let result=match local { Some(port)=>send_endpoint(port,peer.ip,peer.port,true,data), None=>Err(Errno::Inval) };
        net_unlock();result
    }
}

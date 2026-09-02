use crate::net::G_IP;
use crate::net::checksum::pseudo_checksum;
use crate::net::ip;
use crate::net::lock::{net_lock, net_unlock};
use onyx_core::errno::{Errno, KResult};

use super::{
    MAX_UDP_SOCKS, UDP_BUF_SIZE, UDP_HLEN, UDP_SOCKS, UdpSocket, alloc_udp_sock, next_udp_port,
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
        let result = send_packet_inner(socket, data);
        net_unlock();
        result
    }
}

/// Lock-free body of udp_send (caller holds net_lock; may already be nested).
///
/// # Safety
///
/// Caller contract: net_lock() held; `socket` exclusively owned by the caller.
unsafe fn send_packet_inner(socket: &mut UdpSocket, data: &[u8]) -> KResult<()> {
    // SAFETY: segment vector sized UDP_HLEN + data.len(); socket access is through the exclusive &mut above.
    unsafe {
        if !socket.connected {
            return Err(Errno::Inval);
        }
        let udp_len = UDP_HLEN + data.len();
        let mut seg = alloc::vec![0u8; udp_len];
        seg[0..2].copy_from_slice(&socket.local_port.to_be_bytes());
        seg[2..4].copy_from_slice(&socket.remote_port.to_be_bytes());
        seg[4..6].copy_from_slice(&(udp_len as u16).to_be_bytes());
        seg[6..8].copy_from_slice(&[0, 0]);
        if !data.is_empty() {
            seg[UDP_HLEN..].copy_from_slice(data);
        }
        let cksum = pseudo_checksum(&G_IP, &socket.remote_ip, ip::IP_PROTO_UDP, &seg);
        seg[6..8].copy_from_slice(&cksum.to_be_bytes());
        ip::send_packet(socket.remote_ip, ip::IP_PROTO_UDP, &seg)
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
        let idx = alloc_udp_sock().ok_or_else(|| {
            net_unlock();
            Errno::Busy
        })?;
        UDP_SOCKS[idx] = Some(UdpSocket {
            local_port: port,
            remote_ip: [0; 4],
            remote_port: 0,
            bound: true,
            connected: false,
            recv_buf: [0; UDP_BUF_SIZE],
            recv_len: 0,
            recv_head: 0,
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
            local_port: port,
            remote_ip: dst_ip,
            remote_port: dst_port,
            bound: false,
            connected: true,
            recv_buf: [0; UDP_BUF_SIZE],
            recv_len: 0,
            recv_head: 0,
        });
        let result = {
            // Audit fix (🟡 #7): replace `UDP_SOCKS[idx].as_mut().unwrap()`.
            // alloc_udp_sock() returned Some(idx) moments ago, but a
            // malicious caller racing on another core could close the slot
            // between alloc and here. The unwrap would then panic the
            // kernel. Match-and-fail-closed is safe.
            let sock = match UDP_SOCKS[idx].as_mut() {
                Some(s) => s,
                None => {
                    return Err(Errno::Inval);
                }
            };
            udp_send(sock, data)
        };
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
        let port = next_udp_port();
        UDP_SOCKS[idx] = Some(UdpSocket {
            local_port: port,
            remote_ip: dst_ip,
            remote_port: dst_port,
            bound: true,
            connected: true,
            recv_buf: [0; UDP_BUF_SIZE],
            recv_len: 0,
            recv_head: 0,
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
        let result = match UDP_SOCKS[sock_idx].as_mut() {
            Some(s) => send_packet_inner(s, data),
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
    // SAFETY: valid-slot contract on sock_idx; ring reads masked % UDP_BUF_SIZE; head/len advanced under net_lock() so a concurrent poll/handle_udp cannot race the ring (net sync fix, todo P1 #4).
    unsafe {
        net_lock();
        let r = udp_recv_inner(sock_idx, buf);
        net_unlock();
        r
    }
}

/// Lock-free body of udp_recv (caller holds net_lock).
///
/// # Safety
///
/// Caller contract: net_lock() held; valid slot index.
unsafe fn udp_recv_inner(sock_idx: usize, buf: &mut [u8]) -> KResult<usize> {
    // SAFETY: sock_idx is bounds-checked below (a hostile/out-of-range
    // index returns EINVAL instead of indexing out of the slot table and
    // panicking the kernel); ring reads masked % UDP_BUF_SIZE; caller holds
    // net_lock().
    unsafe {
        if sock_idx >= MAX_UDP_SOCKS {
            return Err(Errno::Inval);
        }
        let sock = UDP_SOCKS[sock_idx].as_mut().ok_or(Errno::Inval)?;
        if sock.recv_len == 0 {
            return Err(Errno::NoEnt);
        }
        let n = buf.len().min(sock.recv_len);
        for (i, dst) in buf[..n].iter_mut().enumerate() {
            *dst = sock.recv_buf[(sock.recv_head + i) % UDP_BUF_SIZE];
        }
        sock.recv_head = (sock.recv_head + n) % UDP_BUF_SIZE;
        sock.recv_len -= n;
        Ok(n)
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

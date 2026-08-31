use crate::net::G_IP;
use crate::net::ip;
use crate::net::lock::{net_lock, net_unlock};
use onyx_core::errno::{Errno, KResult};

pub const UDP_HLEN: usize = 8;
pub const MAX_UDP_SOCKS: usize = 8;
pub const UDP_BUF_SIZE: usize = 2048;

#[derive(Clone, Copy)]
pub struct UdpSocket {
    pub local_port: u16,
    pub remote_ip: [u8; 4],
    pub remote_port: u16,
    pub bound: bool,
    pub connected: bool,
    pub recv_buf: [u8; UDP_BUF_SIZE],
    pub recv_len: usize,
    pub recv_head: usize,
}

static mut UDP_SOCKS: [Option<UdpSocket>; MAX_UDP_SOCKS] = [const { None }; MAX_UDP_SOCKS];
static mut NEXT_UDP_PORT: u16 = 50000;

fn alloc_udp_sock() -> Option<usize> {
    // SAFETY: read-only scan of UDP_SOCKS; every slot is always initialized (const { None }).
    unsafe { UDP_SOCKS.iter().position(Option::is_none) }
}

fn next_udp_port() -> u16 {
    // SAFETY: RMW on the NEXT_UDP_PORT static; runs under net_lock() held
    // by every caller (net sync fix, todo P1 #4).
    unsafe {
        let p = NEXT_UDP_PORT;
        NEXT_UDP_PORT = NEXT_UDP_PORT.wrapping_add(1);
        p
    }
}

fn udp_checksum(src_ip: &[u8; 4], dst_ip: &[u8; 4], segment: &[u8]) -> u16 {
    let mut sum = 0u32;
    for i in 0..2 {
        sum = sum.wrapping_add(u16::from_be_bytes([src_ip[i * 2], src_ip[i * 2 + 1]]) as u32);
    }
    for i in 0..2 {
        sum = sum.wrapping_add(u16::from_be_bytes([dst_ip[i * 2], dst_ip[i * 2 + 1]]) as u32);
    }
    sum = sum.wrapping_add(0x0011u32);
    sum = sum.wrapping_add(segment.len() as u32);
    let mut i = 0;
    let pad = if !segment.len().is_multiple_of(2) {
        1
    } else {
        0
    };
    while i + 1 < segment.len() + pad {
        let b0 = if i < segment.len() { segment[i] } else { 0 };
        let b1 = if i + 1 < segment.len() {
            segment[i + 1]
        } else {
            0
        };
        sum = sum.wrapping_add(u16::from_be_bytes([b0, b1]) as u32);
        i += 2;
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16)
}

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
        let cksum = udp_checksum(&G_IP, &socket.remote_ip, &seg);
        seg[6..8].copy_from_slice(&cksum.to_be_bytes());
        ip::send_packet(socket.remote_ip, ip::IP_PROTO_UDP, &seg)
    }
}

/// # Safety
///
/// RX path: iterates the lock-free UDP_SOCKS table and mutates matched
/// slots; must run under the net single-poller contract (no concurrent
/// socket syscalls on another hart).
pub unsafe fn handle_udp(frame: &[u8], ip_start: usize) {
    // SAFETY: offsets bounds-checked before indexing; ring writes masked % UDP_BUF_SIZE; table access under net_lock() (recursive: net::poll already holds it on this hart).
    unsafe {
        net_lock();
        handle_udp_inner(frame, ip_start);
        net_unlock();
    }
}

/// Lock-free body of handle_udp (caller holds net_lock).
///
/// # Safety
///
/// Caller contract: net_lock() held.
unsafe fn handle_udp_inner(frame: &[u8], ip_start: usize) {
    // SAFETY: offsets bounds-checked before indexing; ring writes masked % UDP_BUF_SIZE; net_lock() held by caller.
    unsafe {
        let ip_ihl = (frame[ip_start] & 0x0F) as usize * 4;
        let udp_start = ip_start + ip_ihl;
        if udp_start + UDP_HLEN > frame.len() {
            return;
        }
        let dst_port = u16::from_be_bytes([frame[udp_start], frame[udp_start + 1]]);
        let src_port = u16::from_be_bytes([frame[udp_start + 2], frame[udp_start + 3]]);
        let udp_len = u16::from_be_bytes([frame[udp_start + 4], frame[udp_start + 5]]) as usize;
        let payload_start = udp_start + UDP_HLEN;
        let payload_len = udp_len
            .saturating_sub(UDP_HLEN)
            .min(frame.len().saturating_sub(payload_start));
        for sock in UDP_SOCKS.iter_mut().flatten() {
            if sock.bound && sock.local_port == dst_port {
                let n = payload_len.min(UDP_BUF_SIZE - sock.recv_len);
                let start = (sock.recv_head + sock.recv_len) % UDP_BUF_SIZE;
                for j in 0..n {
                    sock.recv_buf[(start + j) % UDP_BUF_SIZE] = frame[payload_start + j];
                }
                sock.recv_len += n;
                if !sock.connected {
                    sock.remote_ip = [
                        frame[ip_start + 12],
                        frame[ip_start + 13],
                        frame[ip_start + 14],
                        frame[ip_start + 15],
                    ];
                    sock.remote_port = src_port;
                }
                return;
            }
        }
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

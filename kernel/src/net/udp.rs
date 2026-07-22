use crate::net::ip;
use crate::net::poll;
use crate::net::G_IP;
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
    for i in 0..MAX_UDP_SOCKS {
        unsafe {
            if UDP_SOCKS[i].is_none() {
                return Some(i);
            }
        }
    }
    None
}

fn next_udp_port() -> u16 {
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
    let pad = if segment.len() % 2 != 0 { 1 } else { 0 };
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

pub unsafe fn udp_send(socket: &mut UdpSocket, data: &[u8]) -> KResult<()> {
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

pub unsafe fn handle_udp(frame: &[u8], ip_start: usize) {
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
    for i in 0..MAX_UDP_SOCKS {
        if let Some(ref mut sock) = UDP_SOCKS[i] {
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

pub unsafe fn udp_bind(port: u16) -> KResult<usize> {
    let idx = alloc_udp_sock().ok_or(Errno::Busy)?;
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
    Ok(idx)
}

pub unsafe fn udp_sendto(dst_ip: [u8; 4], dst_port: u16, data: &[u8]) -> KResult<()> {
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

pub unsafe fn udp_recv(sock_idx: usize, buf: &mut [u8]) -> KResult<usize> {
    let sock = UDP_SOCKS[sock_idx].as_mut().ok_or(Errno::Inval)?;
    if sock.recv_len == 0 {
        return Err(Errno::NoEnt);
    }
    let n = buf.len().min(sock.recv_len);
    for i in 0..n {
        buf[i] = sock.recv_buf[(sock.recv_head + i) % UDP_BUF_SIZE];
    }
    sock.recv_head = (sock.recv_head + n) % UDP_BUF_SIZE;
    sock.recv_len -= n;
    Ok(n)
}

pub unsafe fn udp_close(sock_idx: usize) {
    UDP_SOCKS[sock_idx] = None;
}

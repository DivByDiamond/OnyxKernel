use crate::net::ip;
use crate::net::G_IP;
use onyx_core::errno::KResult;

pub(super) const MAX_CONNS: usize = 8;
pub(super) const BUF_SIZE: usize = 2048;
pub(super) const TCP_HLEN: usize = 20;

#[derive(Clone, Copy)]
pub(super) struct TcpConn {
    pub(super) state: u8,
    pub(super) src_port: u16,
    pub(super) dst_ip: [u8; 4],
    pub(super) dst_port: u16,
    pub(super) snd_nxt: u32,
    pub(super) rcv_nxt: u32,
    pub(super) send_buf: [u8; BUF_SIZE],
    pub(super) send_len: usize,
    pub(super) recv_buf: [u8; BUF_SIZE],
    pub(super) recv_len: usize,
    pub(super) recv_head: usize,
}

pub(super) static mut CONNS: [Option<TcpConn>; MAX_CONNS] = [None; MAX_CONNS];
static mut NEXT_PORT: u16 = 40000;

fn tcp_checksum(src_ip: &[u8; 4], dst_ip: &[u8; 4], segment: &[u8]) -> u16 {
    let mut sum = 0u32;
    for i in 0..2 {
        sum = sum.wrapping_add(u16::from_be_bytes([src_ip[i * 2], src_ip[i * 2 + 1]]) as u32);
    }
    for i in 0..2 {
        sum = sum.wrapping_add(u16::from_be_bytes([dst_ip[i * 2], dst_ip[i * 2 + 1]]) as u32);
    }
    sum = sum.wrapping_add(0x0006u32);
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

pub(super) fn send_tcp_seg(c: &TcpConn, flags: u8, data: &[u8]) {
    let tcp_len = TCP_HLEN + data.len();
    let mut seg = alloc::vec![0u8; tcp_len];
    seg[0..2].copy_from_slice(&c.src_port.to_be_bytes());
    seg[2..4].copy_from_slice(&c.dst_port.to_be_bytes());
    seg[4..8].copy_from_slice(&c.snd_nxt.to_be_bytes());
    seg[8..12].copy_from_slice(&c.rcv_nxt.to_be_bytes());
    let off_flags = ((TCP_HLEN as u16) / 4) << 12 | flags as u16;
    seg[12..14].copy_from_slice(&off_flags.to_be_bytes());
    seg[14..16].copy_from_slice(&[0xFF, 0xFF]);
    seg[16..18].copy_from_slice(&[0, 0]);
    seg[18..20].copy_from_slice(&[0, 0]);
    if !data.is_empty() {
        seg[TCP_HLEN..].copy_from_slice(data);
    }
    unsafe {
        let cksum = tcp_checksum(&G_IP, &c.dst_ip, &seg);
        seg[16..18].copy_from_slice(&cksum.to_be_bytes());
    }
    unsafe { ip::send_packet(c.dst_ip, 6, &seg) }.ok();
}

pub(super) fn alloc_conn() -> Option<usize> {
    for i in 0..MAX_CONNS {
        unsafe {
            if CONNS[i].is_none() {
                return Some(i);
            }
        }
    }
    None
}

pub(super) fn next_port() -> u16 {
    unsafe {
        let p = NEXT_PORT;
        NEXT_PORT = NEXT_PORT.wrapping_add(1);
        p
    }
}

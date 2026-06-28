use crate::net::ip;
use crate::net::{poll, G_IP};
use onyx_core::errno::{Errno, KResult};
const MAX_CONNS: usize = 8;
const BUF_SIZE: usize = 2048;
const TCP_HLEN: usize = 20;
#[derive(Clone, Copy)]
struct TcpConn {
    state: u8,
    src_port: u16,
    dst_ip: [u8; 4],
    dst_port: u16,
    snd_nxt: u32,
    rcv_nxt: u32,
    send_buf: [u8; BUF_SIZE],
    send_len: usize,
    recv_buf: [u8; BUF_SIZE],
    recv_len: usize,
    recv_head: usize,
}
static mut CONNS: [Option<TcpConn>; MAX_CONNS] = [None; MAX_CONNS];
static mut NEXT_PORT: u16 = 40000;

fn tcp_checksum(src_ip: &[u8; 4], dst_ip: &[u8; 4], segment: &[u8]) -> u16 {
    let mut sum = 0u32;
    for i in 0..2 { sum = sum.wrapping_add(u16::from_be_bytes([src_ip[i * 2], src_ip[i * 2 + 1]]) as u32); }
    for i in 0..2 { sum = sum.wrapping_add(u16::from_be_bytes([dst_ip[i * 2], dst_ip[i * 2 + 1]]) as u32); }
    sum = sum.wrapping_add(0x0006u32);
    sum = sum.wrapping_add(segment.len() as u32);
    let mut i = 0;
    let pad = if segment.len() % 2 != 0 { 1 } else { 0 };
    while i + 1 < segment.len() + pad {
        let b0 = if i < segment.len() { segment[i] } else { 0 };
        let b1 = if i + 1 < segment.len() { segment[i + 1] } else { 0 };
        sum = sum.wrapping_add(u16::from_be_bytes([b0, b1]) as u32);
        i += 2;
    }
    while sum >> 16 != 0 { sum = (sum & 0xFFFF) + (sum >> 16); }
    !(sum as u16)
}

fn send_tcp_seg(c: &TcpConn, flags: u8, data: &[u8]) {
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
    if !data.is_empty() { seg[TCP_HLEN..].copy_from_slice(data); }
    unsafe {
        let cksum = tcp_checksum(&G_IP, &c.dst_ip, &seg);
        seg[16..18].copy_from_slice(&cksum.to_be_bytes());
    }
    unsafe { ip::send_packet(c.dst_ip, 6, &seg) }.ok();
}

fn alloc_conn() -> Option<usize> {
    for i in 0..MAX_CONNS { unsafe { if CONNS[i].is_none() { return Some(i); } } }
    None
}

fn next_port() -> u16 { unsafe { let p = NEXT_PORT; NEXT_PORT = NEXT_PORT.wrapping_add(1); p } }

pub unsafe fn tcp_connect(dst_ip: [u8; 4], port: u16) -> KResult<usize> {
    let cid = alloc_conn().ok_or(Errno::Busy)?;
    let sport = next_port();
    let conn = TcpConn {
        state: 1, src_port: sport, dst_ip, dst_port: port,
        snd_nxt: (sport as u32).wrapping_add(1), rcv_nxt: 0,
        send_buf: [0; BUF_SIZE], send_len: 0, recv_buf: [0; BUF_SIZE], recv_len: 0, recv_head: 0,
    };
    CONNS[cid] = Some(conn);
    if let Some(ref conn) = CONNS[cid] { let _ = send_tcp_seg(conn, 0x02, &[]); }
    for _ in 0..50000 {
        poll();
        if let Some(ref c) = CONNS[cid] { if c.state == 2 { return Ok(cid); } }
    }
    CONNS[cid] = None;
    Err(Errno::Io)
}

pub unsafe fn tcp_send(cid: usize, data: &[u8]) -> KResult<usize> {
    let conn = CONNS[cid].as_mut().ok_or(Errno::Inval)?;
    if conn.state != 2 { return Err(Errno::Io); }
    let n = data.len().min(BUF_SIZE - conn.send_len);
    conn.send_buf[conn.send_len..conn.send_len + n].copy_from_slice(&data[..n]);
    conn.send_len += n;
    let _ = send_tcp_seg(conn, 0x18, &data[..n]);
    conn.snd_nxt = conn.snd_nxt.wrapping_add(n as u32);
    Ok(n)
}

pub unsafe fn tcp_recv(cid: usize, buf: &mut [u8]) -> KResult<usize> {
    let conn = CONNS[cid].as_mut().ok_or(Errno::Inval)?;
    if conn.recv_len == 0 { return Err(Errno::NoEnt); }
    let n = buf.len().min(conn.recv_len);
    for i in 0..n { buf[i] = conn.recv_buf[(conn.recv_head + i) % BUF_SIZE]; }
    conn.recv_head = (conn.recv_head + n) % BUF_SIZE;
    conn.recv_len -= n;
    Ok(n)
}

pub unsafe fn tcp_close(cid: usize) {
    if let Some(conn) = CONNS[cid].as_ref() {
        if conn.state == 2 { let c = *conn; let _ = send_tcp_seg(&c, 0x11, &[]); }
    }
    CONNS[cid] = None;
}

pub unsafe fn handle_tcp(frame: &[u8], ip_start: usize, ihl: usize, total_len: usize) {
    let tcp_off = ip_start + ihl;
    if tcp_off + TCP_HLEN > frame.len() { return; }
    let dport = u16::from_be_bytes([frame[tcp_off], frame[tcp_off + 1]]);
    let _sport = u16::from_be_bytes([frame[tcp_off + 2], frame[tcp_off + 3]]);
    let seq = u32::from_be_bytes([frame[tcp_off + 4], frame[tcp_off + 5], frame[tcp_off + 6], frame[tcp_off + 7]]);
    let ack = u32::from_be_bytes([frame[tcp_off + 8], frame[tcp_off + 9], frame[tcp_off + 10], frame[tcp_off + 11]]);
    let flags = frame[tcp_off + 13];
    let data_off = ((frame[tcp_off + 12] >> 4) as usize) * 4;
    let payload_start = tcp_off + data_off;
    let payload_len = (ip_start + total_len).min(frame.len()).saturating_sub(payload_start);
    for i in 0..MAX_CONNS {
        if let Some(ref mut c) = CONNS[i] {
            if c.src_port != dport { continue; }
            match c.state {
                1 if (flags & 0x12) == 0x12 => {
                    c.state = 2;
                    c.snd_nxt = ack;
                    c.rcv_nxt = seq.wrapping_add(1);
                    send_tcp_seg(c, 0x10, &[]);
                }
                2 => {
                    if seq == c.rcv_nxt && payload_len > 0 {
                        let n = payload_len.min(BUF_SIZE - c.recv_len);
                        let src = &frame[payload_start..payload_start + n];
                        let start = (c.recv_head + c.recv_len) % BUF_SIZE;
                        for j in 0..n { c.recv_buf[(start + j) % BUF_SIZE] = src[j]; }
                        c.recv_len += n;
                        c.rcv_nxt = c.rcv_nxt.wrapping_add(n as u32);
                        send_tcp_seg(c, 0x10, &[]);
                    }
                    if flags & 0x01 != 0 {
                        c.state = 4;
                        c.rcv_nxt = seq.wrapping_add(1);
                        send_tcp_seg(c, 0x11, &[]);
                    }
                }
                3 if flags & 0x10 != 0 => c.state = 4,
                _ => {}
            }
        }
    }
}

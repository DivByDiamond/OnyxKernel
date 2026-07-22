use crate::net::poll;
use onyx_core::errno::{Errno, KResult};

use super::conn::{alloc_conn, next_port, send_tcp_seg, BUF_SIZE, CONNS, TCP_HLEN};

pub unsafe fn tcp_connect(dst_ip: [u8; 4], port: u16) -> KResult<usize> {
    let cid = alloc_conn().ok_or(Errno::Busy)?;
    let sport = next_port();
    let conn = super::conn::TcpConn {
        state: 1,
        src_port: sport,
        dst_ip,
        dst_port: port,
        snd_nxt: (sport as u32).wrapping_add(1),
        rcv_nxt: 0,
        send_buf: [0; BUF_SIZE],
        send_len: 0,
        recv_buf: [0; BUF_SIZE],
        recv_len: 0,
        recv_head: 0,
    };
    CONNS[cid] = Some(conn);
    if let Some(ref conn) = CONNS[cid] {
        let _ = send_tcp_seg(conn, 0x02, &[]);
    }
    for _ in 0..50000 {
        poll();
        if let Some(ref c) = CONNS[cid] {
            if c.state == 2 {
                return Ok(cid);
            }
        }
    }
    CONNS[cid] = None;
    Err(Errno::Io)
}

pub unsafe fn tcp_send(cid: usize, data: &[u8]) -> KResult<usize> {
    let conn = CONNS[cid].as_mut().ok_or(Errno::Inval)?;
    if conn.state != 2 {
        return Err(Errno::Io);
    }
    let n = data.len().min(BUF_SIZE - conn.send_len);
    conn.send_buf[conn.send_len..conn.send_len + n].copy_from_slice(&data[..n]);
    conn.send_len += n;
    let _ = send_tcp_seg(conn, 0x18, &data[..n]);
    conn.snd_nxt = conn.snd_nxt.wrapping_add(n as u32);
    Ok(n)
}

pub unsafe fn tcp_recv(cid: usize, buf: &mut [u8]) -> KResult<usize> {
    let conn = CONNS[cid].as_mut().ok_or(Errno::Inval)?;
    if conn.recv_len == 0 {
        return Err(Errno::NoEnt);
    }
    let n = buf.len().min(conn.recv_len);
    for i in 0..n {
        buf[i] = conn.recv_buf[(conn.recv_head + i) % BUF_SIZE];
    }
    conn.recv_head = (conn.recv_head + n) % BUF_SIZE;
    conn.recv_len -= n;
    Ok(n)
}

pub unsafe fn tcp_close(cid: usize) {
    if let Some(conn) = CONNS[cid].as_ref() {
        if conn.state == 2 {
            let c = *conn;
            let _ = send_tcp_seg(&c, 0x11, &[]);
        }
    }
    CONNS[cid] = None;
}

pub unsafe fn handle_tcp(frame: &[u8], ip_start: usize, ihl: usize, total_len: usize) {
    let tcp_off = ip_start + ihl;
    if tcp_off + TCP_HLEN > frame.len() {
        return;
    }
    let dport = u16::from_be_bytes([frame[tcp_off], frame[tcp_off + 1]]);
    let _sport = u16::from_be_bytes([frame[tcp_off + 2], frame[tcp_off + 3]]);
    let seq = u32::from_be_bytes([
        frame[tcp_off + 4],
        frame[tcp_off + 5],
        frame[tcp_off + 6],
        frame[tcp_off + 7],
    ]);
    let ack = u32::from_be_bytes([
        frame[tcp_off + 8],
        frame[tcp_off + 9],
        frame[tcp_off + 10],
        frame[tcp_off + 11],
    ]);
    let flags = frame[tcp_off + 13];
    let data_off = ((frame[tcp_off + 12] >> 4) as usize) * 4;
    let payload_start = tcp_off + data_off;
    let payload_len = (ip_start + total_len)
        .min(frame.len())
        .saturating_sub(payload_start);
    for i in 0..super::conn::MAX_CONNS {
        if let Some(ref mut c) = CONNS[i] {
            if c.src_port != dport {
                continue;
            }
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
                        for j in 0..n {
                            c.recv_buf[(start + j) % BUF_SIZE] = src[j];
                        }
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

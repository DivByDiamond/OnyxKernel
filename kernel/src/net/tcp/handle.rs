use core::sync::atomic::{AtomicU32, Ordering};

use crate::net::G_IP;
use crate::net::poll;
use onyx_core::errno::{Errno, KResult};

use super::conn::{
    BUF_SIZE, CONNS, TCP_HLEN, TIMEWAIT_US, alloc_conn, alloc_local_port, now_us, send_tcp_seg,
    sweep_timewait, tcp_checksum_ok,
};

const FIN: u8 = 0x01;
const SYN: u8 = 0x02;
const ACK: u8 = 0x10;

/// Total inbound segments dropped for a bad checksum (logged once).
static BAD_CKSUM: AtomicU32 = AtomicU32::new(0);

pub unsafe fn tcp_connect(dst_ip: [u8; 4], port: u16) -> KResult<usize> {
    unsafe {
        let cid = alloc_conn().ok_or(Errno::Busy)?;
        let sport = alloc_local_port();
        let isn = (sport as u32).wrapping_add(1);
        CONNS[cid] = Some(super::conn::TcpConn {
            state: 1,
            src_port: sport,
            dst_ip,
            dst_port: port,
            snd_una: isn,
            snd_nxt: isn,
            rcv_nxt: 0,
            send_buf: [0; BUF_SIZE],
            send_len: 0,
            recv_buf: [0; BUF_SIZE],
            recv_len: 0,
            recv_head: 0,
            tw_deadline_us: 0,
        });
        if let Some(ref conn) = CONNS[cid] {
            send_tcp_seg(conn, SYN, &[]);
        }
        for _ in 0..50000 {
            poll();
            if let Some(ref c) = CONNS[cid]
                && c.state == 2
            {
                return Ok(cid);
            }
        }
        CONNS[cid] = None;
        Err(Errno::Io)
    }
}

pub unsafe fn tcp_send(cid: usize, data: &[u8]) -> KResult<usize> {
    unsafe {
        let conn = CONNS[cid].as_mut().ok_or(Errno::Inval)?;
        if conn.state != 2 {
            return Err(Errno::Io);
        }
        // Space freed as ACKs advance snd_una (drain_acked), so send_len here
        // reflects only bytes the peer has not yet acknowledged.
        let n = data.len().min(BUF_SIZE - conn.send_len);
        conn.send_buf[conn.send_len..conn.send_len + n].copy_from_slice(&data[..n]);
        conn.send_len += n;
        send_tcp_seg(conn, 0x18, &data[..n]);
        conn.snd_nxt = conn.snd_nxt.wrapping_add(n as u32);
        Ok(n)
    }
}

pub unsafe fn tcp_recv(cid: usize, buf: &mut [u8]) -> KResult<usize> {
    unsafe {
        let conn = CONNS[cid].as_mut().ok_or(Errno::Inval)?;
        if conn.recv_len == 0 {
            return Err(Errno::NoEnt);
        }
        let n = buf.len().min(conn.recv_len);
        for (i, dst) in buf[..n].iter_mut().enumerate() {
            *dst = conn.recv_buf[(conn.recv_head + i) % BUF_SIZE];
        }
        conn.recv_head = (conn.recv_head + n) % BUF_SIZE;
        conn.recv_len -= n;
        Ok(n)
    }
}

pub unsafe fn tcp_close(cid: usize) {
    unsafe {
        if let Some(conn) = CONNS[cid].as_ref()
            && conn.state == 2
        {
            let c = *conn;
            send_tcp_seg(&c, FIN | ACK, &[]);
        }
        CONNS[cid] = None;
    }
}

/// Free acked bytes from the send window: advance snd_una and drop the
/// leading `acked` bytes of send_buf. A bogus ACK (outside
/// [snd_una, snd_nxt]) is ignored.
fn drain_acked(c: &mut super::conn::TcpConn, ack: u32) {
    let outstanding = c.snd_nxt.wrapping_sub(c.snd_una);
    let acked = ack.wrapping_sub(c.snd_una);
    if acked == 0 || acked > outstanding || acked as usize > c.send_len {
        return;
    }
    let acked = acked as usize;
    c.send_buf.copy_within(acked..c.send_len, 0);
    c.send_len -= acked;
    c.snd_una = ack;
}

fn process_segment(c: &mut super::conn::TcpConn, seq: u32, ack: u32, flags: u8, payload: &[u8]) {
    match c.state {
        1 if (flags & (SYN | ACK)) == (SYN | ACK) => {
            c.state = 2;
            c.snd_nxt = ack;
            c.snd_una = ack;
            c.rcv_nxt = seq.wrapping_add(1);
            send_tcp_seg(c, ACK, &[]);
        }
        2 | 4 => {
            drain_acked(c, ack);
            // In-sequence data only.
            if seq == c.rcv_nxt && !payload.is_empty() {
                let n = payload.len().min(BUF_SIZE - c.recv_len);
                let start = (c.recv_head + c.recv_len) % BUF_SIZE;
                for (j, &b) in payload[..n].iter().enumerate() {
                    c.recv_buf[(start + j) % BUF_SIZE] = b;
                }
                c.recv_len += n;
                c.rcv_nxt = c.rcv_nxt.wrapping_add(n as u32);
                send_tcp_seg(c, ACK, &[]);
            }
            // FIN must land exactly at rcv_nxt to be accepted.
            if flags & FIN != 0 && seq.wrapping_add(payload.len() as u32) == c.rcv_nxt {
                c.rcv_nxt = c.rcv_nxt.wrapping_add(1);
                if c.state == 2 {
                    c.state = 4;
                    send_tcp_seg(c, FIN | ACK, &[]);
                } else {
                    // Retransmitted FIN during TIMEWAIT: re-ACK it.
                    send_tcp_seg(c, ACK, &[]);
                }
                c.tw_deadline_us = now_us() + TIMEWAIT_US;
            }
        }
        3 if flags & ACK != 0 => {
            drain_acked(c, ack);
            c.state = 4;
            c.tw_deadline_us = now_us() + TIMEWAIT_US;
        }
        _ => {}
    }
}

pub unsafe fn handle_tcp(frame: &[u8], ip_start: usize, ihl: usize, total_len: usize) {
    unsafe {
        sweep_timewait(now_us());
        let tcp_off = ip_start + ihl;
        if tcp_off + TCP_HLEN > frame.len() {
            return;
        }
        let seg_end = (ip_start + total_len).min(frame.len());
        if seg_end <= tcp_off {
            return;
        }
        let mut sip = [0u8; 4];
        sip.copy_from_slice(&frame[ip_start + 12..ip_start + 16]);
        let mut dip = [0u8; 4];
        dip.copy_from_slice(&frame[ip_start + 16..ip_start + 20]);
        // Not addressed to us (IP layer does not filter) — drop before any
        // connection matching or checksum work.
        if dip != G_IP {
            return;
        }
        let sport = u16::from_be_bytes([frame[tcp_off + 2], frame[tcp_off + 3]]);
        let dport = u16::from_be_bytes([frame[tcp_off], frame[tcp_off + 1]]);
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
        if data_off < TCP_HLEN {
            return;
        }
        let payload_start = tcp_off + data_off;
        let payload_len = seg_end.saturating_sub(payload_start);
        // Inbound checksum verification: pseudo-header + header + payload,
        // checksum field included. Bad packets are dropped and counted.
        let seg = &frame[tcp_off..seg_end];
        if !tcp_checksum_ok(&sip, &G_IP, seg) {
            let n = BAD_CKSUM.fetch_add(1, Ordering::Relaxed) + 1;
            if n == 1 {
                crate::kwrn!("tcp", "bad inbound checksum, dropping (counted)");
            }
            return;
        }
        let payload = &frame[payload_start..payload_start + payload_len];
        for c in CONNS.iter_mut().flatten() {
            // Full 4-tuple match: local port AND remote IP:port. Prevents
            // off-path injection and lets duplicate local ports coexist.
            if c.src_port != dport || c.dst_port != sport || c.dst_ip != sip {
                continue;
            }
            process_segment(c, seq, ack, flags, payload);
            break;
        }
    }
}

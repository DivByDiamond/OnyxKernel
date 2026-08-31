use core::sync::atomic::{AtomicU32, Ordering};

use crate::net::G_IP;
use crate::net::lock::{net_lock, net_unlock};
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

/// Segment commands the pure transition core can request from the caller.
pub(super) const SEG_ACK: u8 = 1;
pub(super) const SEG_FIN_ACK: u8 = 2;

/// # Safety: kernel context with SIE clear; the connect (SYN send + ACK
/// wait loop with poll) runs under net_lock(), serializing concurrent TCP
/// operations across harts (net sync fix, todo P1 #4). The lock is held for
/// the whole handshake wait — accepted cost, the old contract required the
/// same exclusivity implicitly.
pub unsafe fn tcp_connect(dst_ip: [u8; 4], port: u16) -> KResult<usize> {
    // SAFETY: slot from alloc_conn, fully initialized before the SYN is sent; CONNS mutations and poll re-entry (recursive) all under net_lock().
    unsafe {
        net_lock();
        let r = tcp_connect_inner(dst_ip, port);
        net_unlock();
        r
    }
}

/// Lock-free body of tcp_connect (caller holds net_lock).
///
/// # Safety
///
/// Caller contract: net_lock() held; poll() re-entry is recursive-safe.
unsafe fn tcp_connect_inner(dst_ip: [u8; 4], port: u16) -> KResult<usize> {
    // SAFETY: slot from alloc_conn, fully initialized before the SYN is sent; net_lock() held by caller.
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

/// # Safety: indexes CONNS[cid] under net_lock() (net sync fix, todo P1 #4) — a concurrent recv/send/close or poll handler on another hart serializes behind the lock instead of racing the slot.
pub unsafe fn tcp_send(cid: usize, data: &[u8]) -> KResult<usize> {
    // SAFETY: conn checked non-empty; copy bounds-checked against BUF_SIZE - send_len; slot access under net_lock().
    unsafe {
        net_lock();
        let r = tcp_send_inner(cid, data);
        net_unlock();
        r
    }
}

/// Lock-free body of tcp_send (caller holds net_lock).
///
/// # Safety
///
/// Caller contract: net_lock() held; valid cid.
unsafe fn tcp_send_inner(cid: usize, data: &[u8]) -> KResult<usize> {
    // SAFETY: conn checked non-empty; copy bounds-checked against BUF_SIZE - send_len; net_lock() held by caller.
    unsafe {
        let conn = CONNS[cid].as_mut().ok_or(Errno::Inval)?;
        if conn.state != 2 {
            return Err(Errno::Io);
        }
        // Send space is freed as ACKs advance snd_una (drain_acked); send_len counts unacked bytes.
        let n = data.len().min(BUF_SIZE - conn.send_len);
        conn.send_buf[conn.send_len..conn.send_len + n].copy_from_slice(&data[..n]);
        conn.send_len += n;
        send_tcp_seg(conn, 0x18, &data[..n]);
        conn.snd_nxt = conn.snd_nxt.wrapping_add(n as u32);
        Ok(n)
    }
}

/// # Safety: indexes CONNS[cid] under net_lock() — a concurrent recv or poll handler would otherwise corrupt the ring indices.
pub unsafe fn tcp_recv(cid: usize, buf: &mut [u8]) -> KResult<usize> {
    // SAFETY: conn checked non-empty; ring reads masked % BUF_SIZE; head/len advanced under net_lock().
    unsafe {
        net_lock();
        let r = tcp_recv_inner(cid, buf);
        net_unlock();
        r
    }
}

/// Lock-free body of tcp_recv (caller holds net_lock).
///
/// # Safety
///
/// Caller contract: net_lock() held; valid cid.
unsafe fn tcp_recv_inner(cid: usize, buf: &mut [u8]) -> KResult<usize> {
    // SAFETY: conn checked non-empty; ring reads masked % BUF_SIZE; net_lock() held by caller.
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

/// # Safety: indexes CONNS[cid] under net_lock() (net sync fix, todo P1 #4).
pub unsafe fn tcp_close(cid: usize) {
    // SAFETY: FIN send reads a copy of the conn; slot cleared exactly once; slot access under net_lock().
    unsafe {
        net_lock();
        if let Some(conn) = CONNS[cid].as_ref()
            && conn.state == 2
        {
            let c = *conn;
            send_tcp_seg(&c, FIN | ACK, &[]);
        }
        CONNS[cid] = None;
        net_unlock();
    }
}

/// Free acked bytes: advance snd_una and drop the leading send_buf bytes; bogus ACKs are ignored.
pub(super) fn drain_acked(c: &mut super::conn::TcpConn, ack: u32) {
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

/// Pure TCP state-machine core (no I/O, host-testable): mutates the conn, returns SEG_* commands to emit in order; `now` is uptime us.
pub(super) fn tcp_transition(
    c: &mut super::conn::TcpConn,
    seq: u32,
    ack: u32,
    flags: u8,
    payload: &[u8],
    now: u64,
) -> [u8; 2] {
    match c.state {
        1 if (flags & (SYN | ACK)) == (SYN | ACK) => {
            c.state = 2;
            c.snd_nxt = ack;
            c.snd_una = ack;
            c.rcv_nxt = seq.wrapping_add(1);
            [SEG_ACK, 0]
        }
        2 | 4 => {
            drain_acked(c, ack);
            let mut out = [0u8; 2];
            if seq == c.rcv_nxt && !payload.is_empty() {
                let n = payload.len().min(BUF_SIZE - c.recv_len);
                let start = (c.recv_head + c.recv_len) % BUF_SIZE;
                for (j, &b) in payload[..n].iter().enumerate() {
                    c.recv_buf[(start + j) % BUF_SIZE] = b;
                }
                c.recv_len += n;
                c.rcv_nxt = c.rcv_nxt.wrapping_add(n as u32);
                out[0] = SEG_ACK;
            }
            // FIN must land exactly at rcv_nxt to be accepted.
            if flags & FIN != 0 && seq.wrapping_add(payload.len() as u32) == c.rcv_nxt {
                c.rcv_nxt = c.rcv_nxt.wrapping_add(1);
                if c.state == 2 {
                    c.state = 4;
                    out[1] = SEG_FIN_ACK;
                } else {
                    // Retransmitted FIN during TIMEWAIT: re-ACK it.
                    out[1] = SEG_ACK;
                }
                c.tw_deadline_us = now + TIMEWAIT_US;
            }
            out
        }
        3 if flags & ACK != 0 => {
            drain_acked(c, ack);
            c.state = 4;
            c.tw_deadline_us = now + TIMEWAIT_US;
            [0, 0]
        }
        _ => [0, 0],
    }
}

fn process_segment(c: &mut super::conn::TcpConn, seq: u32, ack: u32, flags: u8, payload: &[u8]) {
    let out = tcp_transition(c, seq, ack, flags, payload, now_us());
    for cmd in out {
        match cmd {
            SEG_ACK => send_tcp_seg(c, ACK, &[]),
            SEG_FIN_ACK => send_tcp_seg(c, FIN | ACK, &[]),
            _ => {}
        }
    }
}

/// # Safety: RX path — mutates CONNS and sends under net_lock() (recursive
/// when re-entered from poll inside send_packet; net sync fix, todo P1 #4).
pub unsafe fn handle_tcp(frame: &[u8], ip_start: usize, ihl: usize, total_len: usize) {
    // SAFETY: offsets validated against frame.len() before indexing; table mutation under net_lock().
    unsafe {
        net_lock();
        handle_tcp_inner(frame, ip_start, ihl, total_len);
        net_unlock();
    }
}

/// Lock-free body of handle_tcp (caller holds net_lock).
///
/// # Safety
///
/// Caller contract: net_lock() held.
unsafe fn handle_tcp_inner(frame: &[u8], ip_start: usize, ihl: usize, total_len: usize) {
    // SAFETY: offsets validated against frame.len() before indexing; net_lock() held by caller.
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
        // Not addressed to us (IP layer does not filter): drop before matching/checksum work.
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
        // Verify inbound checksum (checksum field included); drop and count bad segments.
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
            // Full 4-tuple match (local port AND remote IP:port) blocks off-path injection.
            if c.src_port != dport || c.dst_port != sport || c.dst_ip != sip {
                continue;
            }
            process_segment(c, seq, ack, flags, payload);
            break;
        }
    }
}

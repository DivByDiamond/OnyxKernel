use crate::net::lock::{net_lock, net_unlock};
use crate::net::poll;
use onyx_core::errno::{Errno, KResult};

use super::conn::{BUF_SIZE, CONNS, TcpConn, alloc_conn, alloc_local_port, send_tcp_seg};

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
        CONNS[cid] = Some(TcpConn {
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
            send_tcp_seg(conn, super::state::SYN, &[]);
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
            send_tcp_seg(&c, super::state::FIN | super::state::ACK, &[]);
        }
        CONNS[cid] = None;
        net_unlock();
    }
}

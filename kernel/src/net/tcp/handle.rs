use core::sync::atomic::{AtomicU32, Ordering};

use crate::net::G_IP;
use crate::net::lock::{net_lock, net_unlock};

use super::conn::{CONNS, TCP_HLEN, now_us, send_tcp_seg, sweep_timewait, tcp_checksum_ok};
use super::state::{ACK, FIN, SEG_ACK, SEG_FIN_ACK, tcp_transition};

/// Total inbound segments dropped for a bad checksum (logged once).
static BAD_CKSUM: AtomicU32 = AtomicU32::new(0);

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
        // TCP header (RFC 793): bytes 0-1 = source port, bytes 2-3 =
        // destination port — this used to be read backwards (same class of
        // bug as the UDP RX handler, see OnyxKernel/todo.md), which broke
        // the 4-tuple connection match below (`c.src_port != dport ||
        // c.dst_port != sport`) for every legitimate inbound segment: a
        // real SYN-ACK's source port (the remote's port) was compared
        // against `c.dst_port` mislabeled as `sport`, but assigned the raw
        // *destination*-port bytes instead. No connection ever matched, so
        // `tcp_connect`'s SYN-ACK wait always timed out even when the peer
        // replied correctly (confirmed on the wire via pcap).
        let sport = u16::from_be_bytes([frame[tcp_off], frame[tcp_off + 1]]);
        let dport = u16::from_be_bytes([frame[tcp_off + 2], frame[tcp_off + 3]]);
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

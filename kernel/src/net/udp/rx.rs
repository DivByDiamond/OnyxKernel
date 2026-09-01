use crate::net::lock::{net_lock, net_unlock};

use super::{UDP_BUF_SIZE, UDP_HLEN, UDP_SOCKS};

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
        // UDP header (RFC 768): bytes 0-1 = source port, bytes 2-3 =
        // destination port — this used to be read backwards (dst_port from
        // the source-port field and vice versa), so `sock.local_port ==
        // dst_port` below compared against the *sender's* port and never
        // matched a bound socket. Confirmed live: a DHCP OFFER reply
        // (src=67 server, dst=68 client) logged as dst_port=67/src_port=68
        // before this fix — exactly the swapped fields — so the client's
        // port-68 socket never received it and dhcp_discover() always fell
        // back to hardcoded defaults despite the reply reaching the RX ring
        // intact (see OnyxKernel/todo.md).
        let src_port = u16::from_be_bytes([frame[udp_start], frame[udp_start + 1]]);
        let dst_port = u16::from_be_bytes([frame[udp_start + 2], frame[udp_start + 3]]);
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

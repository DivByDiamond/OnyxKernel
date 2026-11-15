//! Connection table, checksums, and the TX helper.
//!
//! `sock`  — public socket API (`tcp_connect/send/recv/close`).
//! `state` — pure state-machine transitions, free of IO.
pub(super) mod sock;
pub(super) mod state;

use crate::net::G_IP;
use crate::net::ip;
use crate::net::lock::{net_lock, net_unlock};
use crate::srv::timer;

pub(in crate::net) const MAX_CONNS: usize = 8;

/// Outbound ring: bytes buffered for transmission, freed as ACKs arrive.
/// Bounded by the peer's advertised window anyway, so it stays small.
pub(in crate::net) const SEND_BUF_SIZE: usize = 2048;

/// Inbound ring: bytes accepted from the wire but not yet read by the
/// application. This is also our advertised receive window (see
/// `send_tcp_seg`), so a larger ring lets a peer blast a whole HTTP
/// response before waiting for a window update — 2048 forced a round trip
/// every 2KB. Kept well under the u16 window field and sized so the eight
/// connection slots stay cheap in `.bss` (8 * ~10KB).
pub(in crate::net) const RECV_BUF_SIZE: usize = 8192;

pub(in crate::net) const TCP_HLEN: usize = 20;
/// How long a connection stays in state 4 (TIMEWAIT-equivalent) before
/// its slot is reclaimed by sweep_timewait(). Bounded and short because
/// this stack keeps no retransmission state — anything arriving for an
/// expired slot is dropped by 4-tuple matching anyway.
pub(in crate::net) const TIMEWAIT_US: u64 = 5_000_000;

#[derive(Clone, Copy)]
pub(in crate::net) struct TcpConn {
    pub owner_pid: u32,
    pub(in crate::net) iface: usize,
    pub(in crate::net) listener: usize,
    pub(in crate::net) last_tx_us: u64,
    pub(in crate::net) retries: u8,
    pub(in crate::net) peer_window: u16,
    pub(in crate::net) state: u8,
    pub(in crate::net) src_port: u16,
    pub(in crate::net) dst_ip: [u8; 4],
    pub(in crate::net) dst_port: u16,
    /// Oldest unacknowledged sequence number; send_buf[0] holds its byte.
    pub(in crate::net) snd_una: u32,
    pub(in crate::net) snd_nxt: u32,
    pub(in crate::net) rcv_nxt: u32,
    pub(in crate::net) send_buf: [u8; SEND_BUF_SIZE],
    pub(in crate::net) send_len: usize,
    pub(in crate::net) recv_buf: [u8; RECV_BUF_SIZE],
    pub(in crate::net) recv_len: usize,
    pub(in crate::net) recv_head: usize,
    /// Set by the state machine when the peer's FIN is accepted: once the
    /// recv ring drains, tcp_recv returns Ok(0) (clean EOF) instead of
    /// "no data yet". False for our own active close (TIMEWAIT via state 3).
    pub(in crate::net) peer_fin: bool,
    /// uptime_us() deadline past which a state-4 slot is freed.
    /// 0 = no deadline (connection not in TIMEWAIT).
    pub(in crate::net) tw_deadline_us: u64,
    /// Announced peer MSS from the SYN/SYN-ACK; defaults to TCP_DEFAULT_MSS
    /// if the handshake never carried the MSS option.
    pub(in crate::net) peer_mss: u16,
}

/// Scalar snapshot of the fields the TX path reads.
///
/// `TcpConn` embeds a 2 KiB send ring and an 8 KiB receive ring, so passing
/// it by value (or `*slot`-copying it out of `CONNS`) costs ~10 KB of kernel
/// stack per copy. That is not affordable: the connect/RX path already runs
/// with ~30 KB of stack in use, so such a copy pushed the stack past the
/// 64 KiB kstack and rewrote the `Proc` header below it (found live while
/// bringing `obrowse` up, todo N2). Anything that needs to send after
/// mutating a slot snapshots these scalars instead.
#[derive(Clone, Copy)]
pub(in crate::net) struct TcpTx {
    pub(in crate::net) iface: usize,
    pub(in crate::net) src_port: u16,
    pub(in crate::net) dst_ip: [u8; 4],
    pub(in crate::net) dst_port: u16,
    pub(in crate::net) snd_una: u32,
    pub(in crate::net) snd_nxt: u32,
    pub(in crate::net) rcv_nxt: u32,
    pub(in crate::net) recv_len: usize,
}

impl TcpConn {
    /// Copy out just the scalars `send_tcp_seg_tx` needs.
    pub(in crate::net) fn tx(&self) -> TcpTx {
        TcpTx {
            iface: self.iface,
            src_port: self.src_port,
            dst_ip: self.dst_ip,
            dst_port: self.dst_port,
            snd_una: self.snd_una,
            snd_nxt: self.snd_nxt,
            rcv_nxt: self.rcv_nxt,
            recv_len: self.recv_len,
        }
    }
}

pub(in crate::net) static mut CONNS: [Option<TcpConn>; MAX_CONNS] = [None; MAX_CONNS];

static mut NEXT_PORT: u16 = 40000;

/// TCP checksum over a full segment (header + payload) with the given
/// pseudo-header IPs. On TX the result goes into the checksum field; on
/// RX feeding the whole segment *including* the checksum field must
/// yield 0 for the segment to be valid (see tcp_checksum_ok).
pub(crate) fn tcp_checksum(src_ip: &[u8; 4], dst_ip: &[u8; 4], segment: &[u8]) -> u16 {
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

/// Verify an inbound segment's checksum (the checksum field is part of
/// `segment` and must be left in place).
pub(crate) fn tcp_checksum_ok(src_ip: &[u8; 4], dst_ip: &[u8; 4], segment: &[u8]) -> bool {
    segment.len() >= TCP_HLEN && tcp_checksum(src_ip, dst_ip, segment) == 0
}

/// TX with an already-snapshotted `TcpTx`. Owners of a `&mut TcpConn` into
/// `CONNS` use this so no ~10 KB `TcpConn` value ever lands on the kernel
/// stack (see `TcpTx` for the overflow this fixes).
pub(in crate::net::tcp) fn send_tcp_seg_tx(c: TcpTx, flags: u8, data: &[u8]) {
    // SYN/SYN-ACK segments carry our MSS option (kind=2, len=4) so the peer
    // can stop assuming the RFC 1122 fallback of 536 bytes; tcp_hdr_len()
    // is 24 bytes for those and the bare 20-byte header for everything else.
    let hdr_len = super::control::mss::tcp_hdr_len(flags);
    let tcp_len = hdr_len + data.len();
    let mut seg = alloc::vec![0u8; tcp_len];
    seg[0..2].copy_from_slice(&c.src_port.to_be_bytes());
    seg[2..4].copy_from_slice(&c.dst_port.to_be_bytes());
    seg[4..8].copy_from_slice(&c.snd_nxt.to_be_bytes());
    seg[8..12].copy_from_slice(&c.rcv_nxt.to_be_bytes());
    let off_flags = ((hdr_len as u16) / 4) << 12 | flags as u16;
    seg[12..14].copy_from_slice(&off_flags.to_be_bytes());
    // Advertised receive window = free space in the inbound ring, clamped
    // to the u16 wire field (RECV_BUF_SIZE is far below 65535 today, but a
    // saturating cast keeps a future bump from silently truncating it).
    let window = (RECV_BUF_SIZE - c.recv_len).min(u16::MAX as usize) as u16;
    seg[14..16].copy_from_slice(&window.to_be_bytes());
    seg[16..18].copy_from_slice(&[0, 0]);
    seg[18..20].copy_from_slice(&[0, 0]);
    if hdr_len > TCP_HLEN {
        seg[TCP_HLEN..hdr_len].copy_from_slice(&super::control::mss::mss_option(
            super::control::mss::TCP_MSS as u16,
        ));
    }
    if !data.is_empty() {
        seg[hdr_len..].copy_from_slice(data);
    }
    // SAFETY: caller holds net_lock; the context is restored after TX/ARP poll.
    unsafe {
        crate::net::config::with_interface(c.iface, || {
            let cksum = tcp_checksum(&G_IP, &c.dst_ip, &seg);
            seg[16..18].copy_from_slice(&cksum.to_be_bytes());
            ip::send_packet(c.dst_ip, 6, &seg)
        })
        .ok();
    }
}

pub(in crate::net) fn alloc_conn() -> Option<usize> {
    // SAFETY: read-only scan of CONNS; every slot is always initialized (None or Some).
    unsafe { CONNS.iter().position(Option::is_none) }
}

/// Allocate a local ephemeral port not currently used by any live
/// connection. Blind increment can wrap onto a port already bound by a
/// connection to a different remote (legal in TCP but ambiguous for a
/// matching engine), so scan until a genuinely free port is found.
pub(in crate::net) fn alloc_local_port() -> u16 {
    // SAFETY: RMW on NEXT_PORT plus read of CONNS; callers hold net_lock()
    // (net sync fix, todo P1 #4).
    unsafe {
        for _ in 0..4096 {
            let p = NEXT_PORT;
            NEXT_PORT = NEXT_PORT.wrapping_add(1);
            if !CONNS.iter().flatten().any(|c| c.src_port == p) {
                return p;
            }
        }
        // Table effectively full of distinct ports — give up deterministically.
        NEXT_PORT
    }
}

/// Free every connection whose TIMEWAIT deadline has passed. Called from
/// net poll / packet processing so dead slots cannot accumulate and
/// exhaust MAX_CONNS.
pub(in crate::net::tcp) fn sweep_timewait(now_us: u64) {
    // SAFETY: mutates CONNS slots under net_lock() (recursive for poll/handle_tcp callers; net sync fix, todo P1 #4).
    unsafe {
        net_lock();
        for slot in CONNS.iter_mut() {
            let expired = match slot {
                Some(c) => {
                    ((c.state == 4 && c.recv_len == 0) || c.state == 5 || c.state == 6)
                        && now_us >= c.tw_deadline_us
                        && c.tw_deadline_us != 0
                }
                None => false,
            };
            if expired {
                *slot = None;
            }
        }
        net_unlock();
    }
}

/// Current uptime in microseconds (100 Hz jiffy resolution).
pub(in crate::net::tcp) fn now_us() -> u64 {
    timer::uptime_us()
}

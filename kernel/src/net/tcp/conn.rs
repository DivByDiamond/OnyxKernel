use crate::net::G_IP;
use crate::net::ip;
use crate::srv::timer;

pub(super) const MAX_CONNS: usize = 8;
pub(super) const BUF_SIZE: usize = 2048;
pub(super) const TCP_HLEN: usize = 20;

/// How long a connection stays in state 4 (TIMEWAIT-equivalent) before
/// its slot is reclaimed by sweep_timewait(). Bounded and short because
/// this stack keeps no retransmission state — anything arriving for an
/// expired slot is dropped by 4-tuple matching anyway.
pub(super) const TIMEWAIT_US: u64 = 5_000_000;

#[derive(Clone, Copy)]
pub(super) struct TcpConn {
    pub(super) state: u8,
    pub(super) src_port: u16,
    pub(super) dst_ip: [u8; 4],
    pub(super) dst_port: u16,
    /// Oldest unacknowledged sequence number; send_buf[0] holds its byte.
    pub(super) snd_una: u32,
    pub(super) snd_nxt: u32,
    pub(super) rcv_nxt: u32,
    pub(super) send_buf: [u8; BUF_SIZE],
    pub(super) send_len: usize,
    pub(super) recv_buf: [u8; BUF_SIZE],
    pub(super) recv_len: usize,
    pub(super) recv_head: usize,
    /// uptime_us() deadline past which a state-4 slot is freed.
    /// 0 = no deadline (connection not in TIMEWAIT).
    pub(super) tw_deadline_us: u64,
}

pub(super) static mut CONNS: [Option<TcpConn>; MAX_CONNS] = [None; MAX_CONNS];
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
    // SAFETY: pseudo-header and segment are in-bounds buffers built above; G_IP read per the net contract.
    unsafe {
        let cksum = tcp_checksum(&G_IP, &c.dst_ip, &seg);
        seg[16..18].copy_from_slice(&cksum.to_be_bytes());
    }
    // SAFETY: seg sized TCP_HLEN + data.len(); TX ring contract per ip::send_packet (single-sender).
    unsafe { ip::send_packet(c.dst_ip, 6, &seg) }.ok();
}

pub(super) fn alloc_conn() -> Option<usize> {
    // SAFETY: read-only scan of CONNS; every slot is always initialized (None or Some).
    unsafe { CONNS.iter().position(Option::is_none) }
}

/// Allocate a local ephemeral port not currently used by any live
/// connection. Blind increment can wrap onto a port already bound by a
/// connection to a different remote (legal in TCP but ambiguous for a
/// matching engine), so scan until a genuinely free port is found.
pub(super) fn alloc_local_port() -> u16 {
    // SAFETY: RMW on NEXT_PORT plus read of CONNS -- unguarded statics; single-hart-at-a-time conn-op contract.
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
pub(super) fn sweep_timewait(now_us: u64) {
    // SAFETY: mutates lock-free CONNS slots; runs under the net single-poller contract (poll/tick call sites).
    unsafe {
        for slot in CONNS.iter_mut() {
            let expired = match slot {
                Some(c) => c.state == 4 && now_us >= c.tw_deadline_us && c.tw_deadline_us != 0,
                None => false,
            };
            if expired {
                *slot = None;
            }
        }
    }
}

/// Current uptime in microseconds (100 Hz jiffy resolution).
pub(super) fn now_us() -> u64 {
    timer::uptime_us()
}

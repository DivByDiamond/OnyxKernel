use super::conn::{BUF_SIZE, TIMEWAIT_US};

pub(super) const FIN: u8 = 0x01;
pub(super) const SYN: u8 = 0x02;
pub(super) const ACK: u8 = 0x10;

/// Segment commands the pure transition core can request from the caller.
pub(super) const SEG_ACK: u8 = 1;
pub(super) const SEG_FIN_ACK: u8 = 2;

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

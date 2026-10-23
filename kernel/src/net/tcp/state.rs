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
    if acked == 0 || acked > outstanding { return; }
    let fin_ack = c.state==3 && ack==c.snd_nxt && acked as usize==c.send_len+1;
    if acked as usize>c.send_len && !fin_ack { return; }
    let acked = (acked as usize).min(c.send_len);
    c.send_buf.copy_within(acked..c.send_len, 0);
    c.send_len -= acked;
    c.snd_una = ack;
    c.retries=0;
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
        1 if (flags & (SYN | ACK)) == (SYN | ACK) && ack == c.snd_nxt => {
            c.state = 2;
            c.snd_nxt = ack;
            c.snd_una = ack;
            c.rcv_nxt = seq.wrapping_add(1);
            [SEG_ACK, 0]
        }
        5 if flags & ACK != 0 && ack == c.snd_nxt && seq == c.rcv_nxt => {
            c.state=2; c.snd_una=ack; c.tw_deadline_us=0;
            tcp_transition(c,seq,ack,flags,payload,now)
        },
        2 | 4 | 7 => {
            drain_acked(c, ack);
            // ACK duplicate/out-of-order data too: this includes zero-window
            // persist probes and lets peers recover from a lost window update.
            let mut out = [if payload.is_empty() {0} else {SEG_ACK}, 0];
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
                c.peer_fin = true;
                if c.state == 2 {
                    // CLOSE_WAIT: peer ended its write side; application may
                    // still send a response until it explicitly closes.
                    c.state = 7;
                    out[1] = SEG_ACK;
                } else { out[1] = SEG_ACK; }
                if c.state == 4 { c.tw_deadline_us = now + TIMEWAIT_US; }
            }
            out
        }
        3 if flags & ACK != 0 => {
            drain_acked(c, ack);
            if ack==c.snd_nxt { c.state = 4; c.tw_deadline_us = now + TIMEWAIT_US; }
            [0, 0]
        }
        _ => [0, 0],
    }
}

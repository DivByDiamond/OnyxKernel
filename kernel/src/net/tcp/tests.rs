use super::conn::{
    BUF_SIZE, CONNS, TIMEWAIT_US, TcpConn, alloc_conn, alloc_local_port, sweep_timewait,
};
use super::state::{SEG_ACK, SEG_FIN_ACK, drain_acked, tcp_transition};

fn test_conn() -> TcpConn {
    // State 1 = SYN_SENT, as left by tcp_connect().
    TcpConn {
        state: 1,
        src_port: 40000,
        dst_ip: [10, 0, 0, 1],
        dst_port: 80,
        snd_una: 1000,
        snd_nxt: 1001,
        rcv_nxt: 0,
        send_buf: [0; BUF_SIZE],
        send_len: 0,
        recv_buf: [0; BUF_SIZE],
        recv_len: 0,
        recv_head: 0,
        tw_deadline_us: 0,
    }
}

#[test]
fn test_handshake_syn_sent_to_established() {
    let mut c = test_conn();
    // SYN|ACK completes the handshake: ESTABLISHED with peer seq+1 as rcv_nxt.
    let out = tcp_transition(&mut c, 5000, 1001, 0x12, &[], 0);
    assert_eq!(c.state, 2);
    assert_eq!(c.snd_una, 1001);
    assert_eq!(c.snd_nxt, 1001);
    assert_eq!(c.rcv_nxt, 5001);
    assert_eq!(out, [SEG_ACK, 0]);
    // A bare SYN or bare ACK in SYN_SENT must not establish.
    let mut c = test_conn();
    assert_eq!(tcp_transition(&mut c, 5000, 1001, 0x02, &[], 0), [0, 0]);
    assert_eq!(c.state, 1);
    assert_eq!(tcp_transition(&mut c, 5000, 1001, 0x10, &[], 0), [0, 0]);
    assert_eq!(c.state, 1);
}

#[test]
fn test_established_in_sequence_data_and_rcv_nxt() {
    let mut c = test_conn();
    tcp_transition(&mut c, 5000, 1001, 0x12, &[], 0); // -> ESTABLISHED, rcv_nxt=5001
    // In-sequence payload is buffered and acked; rcv_nxt advances.
    let out = tcp_transition(&mut c, 5001, 1001, 0x10, b"hello", 0);
    assert_eq!(out, [SEG_ACK, 0]);
    assert_eq!(&c.recv_buf[..5], b"hello");
    assert_eq!(c.recv_len, 5);
    assert_eq!(c.rcv_nxt, 5006);
    // Out-of-sequence (old) data is dropped: no ack, no buffer growth.
    let out = tcp_transition(&mut c, 5001, 1001, 0x10, b"junk", 0);
    assert_eq!(out, [0, 0]);
    assert_eq!(c.recv_len, 5);
    assert_eq!(c.rcv_nxt, 5006);
    // In-sequence again after the drop.
    let out = tcp_transition(&mut c, 5006, 1001, 0x10, b"!", 0);
    assert_eq!(out, [SEG_ACK, 0]);
    assert_eq!(c.rcv_nxt, 5007);
}

#[test]
fn test_fin_sequence_established_to_timewait() {
    let mut c = test_conn();
    tcp_transition(&mut c, 5000, 1001, 0x12, &[], 0);
    // FIN landing exactly at rcv_nxt closes our receive side: TIMEWAIT
    // deadline set and FIN|ACK emitted.
    let out = tcp_transition(&mut c, 5001, 1001, 0x11, &[], 777);
    assert_eq!(c.state, 4);
    assert_eq!(out, [0, SEG_FIN_ACK]);
    assert_eq!(c.rcv_nxt, 5002);
    assert_eq!(c.tw_deadline_us, 777 + TIMEWAIT_US);
    // A stale retransmitted FIN (seq no longer matches rcv_nxt) during
    // TIMEWAIT is ignored: no re-ACK and no state/deadline change.
    let out = tcp_transition(&mut c, 5001, 1001, 0x11, &[], 778);
    assert_eq!(c.state, 4);
    assert_eq!(out, [0, 0]);
    assert_eq!(c.tw_deadline_us, 777 + TIMEWAIT_US);
    // FIN before all data is consumed (seq != rcv_nxt) is ignored.
    let mut c = test_conn();
    tcp_transition(&mut c, 5000, 1001, 0x12, &[], 0);
    assert_eq!(tcp_transition(&mut c, 5002, 1001, 0x11, &[], 0), [0, 0]);
    assert_eq!(c.state, 2);
    // Data + FIN in one segment: plain ACK for the data, then FIN|ACK.
    let mut c = test_conn();
    tcp_transition(&mut c, 5000, 1001, 0x12, &[], 0);
    let out = tcp_transition(&mut c, 5001, 1001, 0x11, b"bye", 0);
    assert_eq!(out, [SEG_ACK, SEG_FIN_ACK]);
    assert_eq!(c.state, 4);
    assert_eq!(c.rcv_nxt, 5005);
}

#[test]
fn test_state3_ack_to_timewait_and_unexpected_segments() {
    // State 3 (our close outstanding): a bare ACK without FIN moves to TIMEWAIT.
    let mut c = test_conn();
    c.state = 3;
    c.send_len = 10;
    c.snd_una = 1000;
    c.snd_nxt = 1010;
    assert_eq!(tcp_transition(&mut c, 0, 1005, 0x10, &[], 42), [0, 0]);
    assert_eq!(c.state, 4);
    assert_eq!(c.tw_deadline_us, 42 + TIMEWAIT_US);
    assert_eq!(c.send_len, 5); // drain_acked freed the acked prefix
    // State 3 without ACK flag: no transition.
    let mut c = test_conn();
    c.state = 3;
    assert_eq!(tcp_transition(&mut c, 0, 1005, 0x00, &[], 0), [0, 0]);
    assert_eq!(c.state, 3);
}

#[test]
fn test_drain_acked_window() {
    let mut c = test_conn();
    c.state = 2;
    c.snd_una = 100;
    c.snd_nxt = 160;
    c.send_buf[..50].copy_from_slice(&[0xAA; 50]);
    c.send_len = 50;
    // Bogus ACKs (equal to snd_una, beyond snd_nxt, or beyond buffered
    // bytes) are ignored.
    drain_acked(&mut c, 100);
    assert_eq!((c.snd_una, c.send_len), (100, 50));
    drain_acked(&mut c, 161);
    assert_eq!((c.snd_una, c.send_len), (100, 50));
    drain_acked(&mut c, 151);
    assert_eq!((c.snd_una, c.send_len), (100, 50));
    // A valid partial ACK drops only the leading acked bytes.
    drain_acked(&mut c, 130);
    assert_eq!((c.snd_una, c.send_len), (130, 20));
    assert_eq!(c.send_buf[0], 0xAA); // remaining bytes shifted to the front
}

#[test]
fn test_conn_table_alloc_port_sweep() {
    // Sole owner of the process-global CONNS table within this test binary:
    // run every table-level assertion in one function to stay race-free
    // under the parallel test harness.
    // SAFETY: sole owner of the process-global CONNS table within this test binary (see above);
    // every slot is initialized, and indices come from alloc_conn.
    unsafe {
        let cid = alloc_conn().expect("a free slot exists");
        let sport = alloc_local_port();
        CONNS[cid] = Some(TcpConn {
            state: 2,
            src_port: sport,
            dst_ip: [10, 0, 0, 1],
            dst_port: 80,
            snd_una: 0,
            snd_nxt: 0,
            rcv_nxt: 0,
            send_buf: [0; BUF_SIZE],
            send_len: 0,
            recv_buf: [0; BUF_SIZE],
            recv_len: 0,
            recv_head: 0,
            tw_deadline_us: 0,
        });
        // alloc_local_port never hands out a port already in use.
        assert_ne!(alloc_local_port(), sport);
        // A live (state 2) connection survives the sweep; an expired state-4
        // slot is freed exactly at its deadline.
        sweep_timewait(0);
        assert!(CONNS[cid].is_some());
        let conn = CONNS[cid].as_mut().expect("live conn");
        conn.state = 4;
        conn.tw_deadline_us = 1_000;
        sweep_timewait(999);
        assert!(CONNS[cid].is_some());
        sweep_timewait(1_000);
        assert!(CONNS[cid].is_none());
        CONNS[cid] = None;
    }
}

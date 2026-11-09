//! Socket-table tests: allocation, sweeping, and send-path clamping.

use super::super::conn::{
    CONNS, RECV_BUF_SIZE, SEND_BUF_SIZE, TcpConn, alloc_conn, alloc_local_port, sweep_timewait,
};
use super::super::control::mss::TCP_MSS;
use super::test_conn;

#[test]
fn test_conn_table_alloc_port_sweep() {
    let _serial = crate::net::lock::test_serial();
    // Sole owner of the process-global CONNS table within this test binary:
    // run every table-level assertion in one function to stay race-free
    // under the parallel test harness.
    // SAFETY: sole owner of the process-global CONNS table within this test binary (see above);
    // every slot is initialized, and indices come from alloc_conn.
    unsafe {
        let cid = alloc_conn().expect("a free slot exists");
        let sport = alloc_local_port();
        CONNS[cid] = Some(TcpConn {
            owner_pid: 0,
            iface: 0,
            listener: usize::MAX,
            last_tx_us: 0,
            retries: 0,
            peer_window: 2048,
            state: 2,
            src_port: sport,
            dst_ip: [10, 0, 0, 1],
            dst_port: 80,
            snd_una: 0,
            snd_nxt: 0,
            rcv_nxt: 0,
            send_buf: [0; SEND_BUF_SIZE],
            send_len: 0,
            recv_buf: [0; RECV_BUF_SIZE],
            recv_len: 0,
            recv_head: 0,
            peer_fin: false,
            tw_deadline_us: 0,
            peer_mss: TCP_MSS as u16,
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

#[test]
fn zero_window_send_does_not_postpone_outstanding_data_retries() {
    let _serial = crate::net::lock::test_serial();
    unsafe {
        crate::net::lock::net_lock();
        let saved = CONNS;
        let mut c = test_conn();
        c.state = 2;
        c.peer_window = 0;
        c.send_len = 1;
        c.last_tx_us = 123;
        c.retries = 2;
        CONNS[0] = Some(c);
        assert_eq!(super::super::conn::sock::tcp_send(0, b"more"), Ok(0));
        assert_eq!(CONNS[0].unwrap().last_tx_us, 123);
        assert_eq!(CONNS[0].unwrap().retries, 2);
        CONNS = saved;
        crate::net::lock::net_unlock();
    }
}

#[test]
fn send_is_clamped_to_the_peer_announced_mss() {
    let _serial = crate::net::lock::test_serial();
    unsafe {
        crate::net::lock::net_lock();
        let saved = CONNS;
        let mut c = test_conn();
        c.state = 2;
        // A peer that announced MSS=100 (or fell back to the RFC default)
        // must never receive a larger segment than it accepted.
        c.peer_mss = 100;
        c.peer_window = 2048;
        CONNS[0] = Some(c);
        assert_eq!(super::super::conn::sock::tcp_send(0, &[b'x'; 500]), Ok(100));
        assert_eq!(CONNS[0].unwrap().send_len, 100);
        // The unclamped path still works for the default1460.
        CONNS[0].as_mut().unwrap().peer_mss = TCP_MSS as u16;
        assert_eq!(super::super::conn::sock::tcp_send(0, &[b'y'; 500]), Ok(500));
        CONNS = saved;
        crate::net::lock::net_unlock();
    }
}

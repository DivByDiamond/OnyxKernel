//! Receive-ring tests: clean EOF after the peer's FIN, and the ring's
//! wrap-around behaviour at the larger `RECV_BUF_SIZE`.
//!
//! Split out of the main test module, which is already at the project's
//! 250-line per-file ceiling.

use onyx_core::errno::Errno;

use super::super::conn::RECV_BUF_SIZE;
use super::super::conn::sock::read_received;
use super::super::conn::state::{SEG_ACK, tcp_transition};
use super::test_conn;

#[test]
fn eof_after_peer_fin_once_the_receive_ring_drains() {
    let _serial = crate::net::lock::test_serial();
    let mut c = test_conn();
    tcp_transition(&mut c, 5000, 1001, 0x12, &[], 0); // -> ESTABLISHED
    let mut out = [0u8; 16];
    // Nothing buffered and no FIN: "not ready yet", never a false EOF.
    assert_eq!(read_received(&mut c, &mut out), Err(Errno::NoEnt));
    // Data + FIN in one segment: the payload is buffered and the FIN accepted.
    assert_eq!(
        tcp_transition(&mut c, 5001, 1001, 0x11, b"bye", 0),
        [SEG_ACK, SEG_ACK]
    );
    assert!(c.peer_fin);
    assert_eq!(c.state, 7); // CLOSE_WAIT: we may still send until we close
    // The buffered payload is delivered first, without a window update —
    // the peer already ended its write side, there is nothing to window.
    let (n, notify) = read_received(&mut c, &mut out).unwrap();
    assert_eq!(&out[..n], b"bye");
    assert!(!notify);
    // Then a stable, repeatable Ok(0): clean EOF, distinguishable from
    // "no data yet" (Err(NoEnt)) and reported to userspace as read()==0.
    assert_eq!(read_received(&mut c, &mut out), Ok((0, false)));
    assert_eq!(read_received(&mut c, &mut out), Ok((0, false)));
}

#[test]
fn receive_ring_wraps_around_the_end_of_the_buffer() {
    let _serial = crate::net::lock::test_serial();
    let mut c = test_conn();
    c.state = 2;
    c.rcv_nxt = 5000;
    // 92 bytes of history already consumed: the next payload straddles the
    // physical end of the array, which is exactly what the modulo ring must
    // handle once the buffer size changes.
    c.recv_head = RECV_BUF_SIZE - 92;
    assert_eq!(
        tcp_transition(&mut c, 5000, 1001, 0x10, &[b'a'; 100], 0),
        [SEG_ACK, 0]
    );
    assert_eq!(c.recv_len, 100);
    let mut out = [0u8; 64];
    let (n, _) = read_received(&mut c, &mut out).unwrap();
    assert_eq!(n, 64);
    assert!(out[..64].iter().all(|&b| b == b'a'));
    // 36 `a`s left at the tail; the next payload lands at the head.
    assert_eq!(
        tcp_transition(&mut c, 5100, 1001, 0x10, &[b'b'; 40], 0),
        [SEG_ACK, 0]
    );
    assert_eq!(c.recv_len, 76);
    let mut out = [0u8; 128];
    let (n, _) = read_received(&mut c, &mut out).unwrap();
    assert_eq!(n, 76);
    assert!(out[..36].iter().all(|&b| b == b'a'));
    assert!(out[36..76].iter().all(|&b| b == b'b'));
    // Drained, no FIN: "no data yet", not EOF.
    assert_eq!(read_received(&mut c, &mut out), Err(Errno::NoEnt));
}

#[test]
fn receive_window_advertises_the_whole_free_ring() {
    let _serial = crate::net::lock::test_serial();
    let mut c = test_conn();
    c.state = 2;
    c.rcv_nxt = 5000;
    // The advertised window is the free space in the inbound ring — the
    // send path clamps it to u16, and RECV_BUF_SIZE must stay in range so
    // the clamp is never what decides the value on the wire.
    assert!(RECV_BUF_SIZE <= u16::MAX as usize);
    tcp_transition(&mut c, 5000, 1001, 0x10, &[0u8; 1024], 0);
    assert_eq!(RECV_BUF_SIZE - c.recv_len, RECV_BUF_SIZE - 1024);
    // A full ring advertises a zero window, which is what stops the peer
    // instead of silently dropping its payload.
    tcp_transition(&mut c, 6024, 1001, 0x10, &[0u8; RECV_BUF_SIZE - 1024], 0);
    assert_eq!(c.recv_len, RECV_BUF_SIZE);
}

use super::*;
#[test]
fn messages_do_not_merge_and_truncation_discards_only_one_datagram() {
    let _serial = crate::net::lock::test_serial();
    let mut q = DatagramQueue::EMPTY;
    let p = Peer {
        ip: [192, 168, 50, 2],
        port: 123,
        iface: 1,
    };
    assert!(q.push(b"hello", p));
    assert!(q.push(b"world", p));
    let mut small = [0; 2];
    assert_eq!(q.pop(&mut small), Some((2, p)));
    assert_eq!(&small, b"he");
    let mut out = [0; 8];
    assert_eq!(q.pop(&mut out), Some((5, p)));
    assert_eq!(&out[..5], b"world");
    assert_eq!(q.pop(&mut out), None);
}
#[test]
fn empty_datagrams_and_full_queue_are_bounded() {
    let _serial = crate::net::lock::test_serial();
    let mut q = DatagramQueue::EMPTY;
    for _ in 0..8 {
        assert!(q.push(&[], EMPTY_PEER));
    }
    assert!(!q.push(b"overflow", EMPTY_PEER));
    assert_eq!(q.pop(&mut []), Some((0, EMPTY_PEER)));
    assert!(q.push(b"ok", EMPTY_PEER));
}

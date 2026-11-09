use super::*;

#[test]
fn ring_is_fifo() {
    let mut r = Ring::new();
    assert!(r.is_empty());
    assert!(r.push(b'a'));
    assert!(r.push(b'b'));
    assert_eq!(r.len(), 2);
    assert_eq!(r.pop(), Some(b'a'));
    assert_eq!(r.pop(), Some(b'b'));
    assert_eq!(r.pop(), None);
    assert!(r.is_empty());
}

#[test]
fn ring_rejects_when_full() {
    let mut r = Ring::new();
    // One slot stays free so head == tail can mean empty.
    for _ in 0..RX_RING_SIZE - 1 {
        assert!(r.push(1));
    }
    assert_eq!(r.len(), RX_RING_SIZE - 1);
    assert!(!r.push(2));
    assert_eq!(r.len(), RX_RING_SIZE - 1);
    // Draining one slot makes room again.
    assert_eq!(r.pop(), Some(1));
    assert!(r.push(3));
}

#[test]
fn ring_wraps_across_the_end() {
    let mut r = Ring::new();
    for i in 0..RX_RING_SIZE - 1 {
        assert!(r.push(i as u8));
    }
    for i in 0..RX_RING_SIZE - 1 {
        assert_eq!(r.pop(), Some(i as u8));
    }
    // head/tail are back at the start, but with the wrap the next push
    // must land at index head, not overflow the buffer.
    for i in 0..100u8 {
        assert!(r.push(i));
    }
    for i in 0..100u8 {
        assert_eq!(r.pop(), Some(i));
    }
}

use super::*;
#[test]
fn receive_consumes_each_entry_and_wraps_at_queue_size() {
    assert_eq!(receive_cursor(0, 3), Some((0, 1)));
    assert_eq!(receive_cursor(1, 3), Some((1, 2)));
    assert_eq!(receive_cursor(16, 17), Some((16, 17)));
    assert_eq!(receive_cursor(255, 256), Some((255, 256)));
    assert_eq!(receive_cursor(u16::MAX, 0), Some((255, 0)));
    assert_eq!(receive_cursor(3, 3), None);
}

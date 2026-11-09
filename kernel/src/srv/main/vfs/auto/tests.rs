use super::*;

#[test]
fn builds_mnt_path_for_single_and_multi_digit_devices() {
    let mut buf = [0u8; 16];
    assert_eq!(build_mnt_path(0, &mut buf), Some(9));
    assert_eq!(&buf[..9], b"/mnt/blk0");
    assert_eq!(build_mnt_path(123, &mut buf), Some(11));
    assert_eq!(&buf[..11], b"/mnt/blk123");
}

#[test]
fn rejects_too_many_digits() {
    let mut buf = [0u8; 16];
    assert_eq!(build_mnt_path(10000, &mut buf), None);
}

use super::*;
#[test]
fn test_be() {
    assert_eq!(be32(&[0x12, 0x34, 0x56, 0x78]), 0x12345678);
}
#[test]
fn test_le() {
    assert_eq!(le32(&[0x78, 0x56, 0x34, 0x12]), 0x12345678);
}
#[test]
fn test_cursor() {
    let buf = [0u8, 1, 2, 3, 4, 5, 6, 7];
    let mut c = Cursor::new(&buf);
    assert_eq!(c.read_u8(), Some(0));
    assert_eq!(c.read_le32(), Some(0x04030201));
    assert_eq!(c.read_le32(), None);
}

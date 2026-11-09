use super::build_dirent;

#[test]
fn dirent_record_layout_matches_linux_dirent64() {
    let mut rec = [0u8; 288];
    let n = build_dirent(&mut rec, 0x1234, 7, 4, b"sub");
    // 19 header bytes + name + NUL, rounded up to 8.
    assert_eq!(n, 24);
    assert_eq!(&rec[0..8], &[0x34, 0x12, 0, 0, 0, 0, 0, 0]);
    // d_off is the cookie of the next entry, not zero.
    assert_eq!(&rec[8..16], &7u64.to_le_bytes());
    assert_eq!(&rec[16..18], &24u16.to_le_bytes());
    assert_eq!(rec[18], 4); // d_type = DT_DIR
    assert_eq!(&rec[19..22], b"sub");
    assert_eq!(rec[22], 0); // name is NUL-terminated inside the record
    assert_eq!(n % 8, 0);
}

#[test]
fn a_name_on_an_8_byte_boundary_still_gets_a_nul() {
    // 19 + 8 + 1 = 28 -> rounded to 32; the NUL must land at index 27,
    // inside the record, instead of bleeding into the next one.
    let mut rec = [0u8; 288];
    let n = build_dirent(&mut rec, 1, 1, 8, b"12345678");
    assert_eq!(n, 32);
    assert_eq!(rec[19 + 8], 0);
    assert!(rec[28..32].iter().all(|&b| b == 0));
}

#[test]
fn a_long_name_stays_within_the_record_buffer() {
    let mut rec = [0u8; 288];
    let name = [b'a'; 255];
    let n = build_dirent(&mut rec, 9, 3, 8, &name);
    assert_eq!(n, 280); // 19 + 255 + 1 = 275 -> 280
    assert_eq!(rec[19 + 255], 0);
    assert!(n <= rec.len());
}

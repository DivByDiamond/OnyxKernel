//! Tests for `crypto::kdf`: hex/field codecs, iterated vs legacy scheme
//! classification, malformed-input rejection and the empty-password edge
//! case. Sibling file keeps kdf.rs within the 250-line file limit.

use super::*;

#[test]
fn hex_roundtrip() {
    let h = sha256(b"abc");
    let mut data = [0u8; 84];
    assert_eq!(format_shadow_field(&[7u8; 8], &h, &mut data), 84);
    let f = parse_shadow_field(&data).unwrap();
    assert_eq!(f.hash, h);
    assert_eq!(f.salt, [7u8; 8]);
}

#[test]
fn format_field_layout() {
    let mut buf = [0u8; 128];
    let n = format_shadow_field(&[0xAB; 8], &sha256(b"x"), &mut buf);
    assert_eq!(n, 84);
    assert_eq!(&buf[..4], b"$5$a");
    assert_eq!(buf[19], b'$');
    // Too-small buffers must fail cleanly, not truncate.
    assert_eq!(
        format_shadow_field(&[0; 8], &sha256(b"x"), &mut [0u8; 83]),
        0
    );
}

#[test]
fn classify_iterated_and_legacy_vectors() {
    let mut data = [0u8; 84];
    let salt = [0x11u8; 8];

    format_shadow_field(&salt, &hash_password(b"s3cret", &salt), &mut data);
    let f = parse_shadow_field(&data).unwrap();
    assert_eq!(classify_password(b"s3cret", &f), HashScheme::Iterated);

    // Legacy vector: SHA256("test" || 0011223344556677), computed with
    // an external reference implementation.
    let lsalt = hex_decode_8(b"0011223344556677");
    let lhash = legacy_hash_password(b"test", &lsalt);
    assert_eq!(
        bytes_to_hex(&lhash)[..64].to_vec(),
        b"4960f822570f0bdc25294b5bd5adef20ecd20516a5beb343720ec789b9364549".to_vec()
    );
    let mut lb = [0u8; 84];
    format_shadow_field(&lsalt, &lhash, &mut lb);
    let lf = parse_shadow_field(&lb).unwrap();
    assert_eq!(classify_password(b"test", &lf), HashScheme::Legacy);
    assert_eq!(classify_password(b"wrong", &lf), HashScheme::Unknown);
}

#[test]
fn parse_rejects_malformed() {
    assert!(parse_shadow_field(b"").is_none());
    assert!(parse_shadow_field(b"plain").is_none());
    assert!(parse_shadow_field(b"$6$0011223344556677$").is_none());
    assert!(parse_shadow_field(b"$5$short$").is_none());
    assert!(parse_shadow_field(b"$5$zz11223344556677$").is_none()); // non-hex salt
    // Truncated hash tail.
    assert!(parse_shadow_field(b"$5$0011223344556677$abcd").is_none());
}

/// Empty-password edge case: accounts seeded with an empty password
/// (first-boot root) must verify against empty input under both the
/// current scheme and the legacy migration path.
#[test]
fn empty_password_verifies() {
    let salt = [0x42u8; 8];
    let mut data = [0u8; 84];
    format_shadow_field(&salt, &hash_password(b"", &salt), &mut data);
    let f = parse_shadow_field(&data).unwrap();
    assert_eq!(classify_password(b"", &f), HashScheme::Iterated);

    format_shadow_field(&salt, &legacy_hash_password(b"", &salt), &mut data);
    let lf = parse_shadow_field(&data).unwrap();
    assert_eq!(classify_password(b"", &lf), HashScheme::Legacy);
}

#[test]
fn const_time_eq_basics() {
    assert!(const_time_eq(b"", b""));
    assert!(const_time_eq(b"abc", b"abc"));
    assert!(!const_time_eq(b"abc", b"abd"));
    assert!(!const_time_eq(b"abc", b"ab"));
}

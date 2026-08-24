use super::eth::{arp_insert, arp_lookup};
use super::ip::checksum;
use super::tcp::conn::tcp_checksum_ok;

#[test]
fn test_tcp_checksum_verify_roundtrip() {
    let src = [10, 0, 0, 2];
    let dst = [10, 0, 0, 5];
    // Minimal TCP header with a correct checksum field.
    let mut seg = [0u8; 20];
    seg[14..16].copy_from_slice(&[0x50, 0x10]); // data-off 5, flags ACK
    let ck = super::tcp::conn::tcp_checksum(&src, &dst, &seg);
    seg[16..18].copy_from_slice(&ck.to_be_bytes());
    assert!(tcp_checksum_ok(&src, &dst, &seg));
}

#[test]
fn test_tcp_checksum_verify_rejects_corruption() {
    let src = [10, 0, 0, 2];
    let dst = [10, 0, 0, 5];
    let mut seg = [0u8; 25]; // odd length exercises the pad path
    seg[24] = 0xAB;
    let ck = super::tcp::conn::tcp_checksum(&src, &dst, &seg);
    seg[16..18].copy_from_slice(&ck.to_be_bytes());
    assert!(tcp_checksum_ok(&src, &dst, &seg));
    // Flip one payload byte — verification must fail.
    seg[24] ^= 0xFF;
    assert!(!tcp_checksum_ok(&src, &dst, &seg));
    // Wrong pseudo-header IP must also fail.
    seg[24] ^= 0xFF;
    assert!(!tcp_checksum_ok(&src, &[10, 0, 0, 6], &seg));
    // Too-short segment is rejected outright.
    assert!(!tcp_checksum_ok(&src, &dst, &[0u8; 8]));
}

#[test]
fn test_checksum_all_zeros() {
    assert_eq!(checksum(&[]), 0xFFFF);
    assert_eq!(checksum(&[0, 0]), 0xFFFF);
    assert_eq!(checksum(&[0, 0, 0, 0]), 0xFFFF);
}

#[test]
fn test_checksum_single_word() {
    assert_eq!(checksum(&[0x00, 0x01]), 0xFFFE);
    assert_eq!(checksum(&[0xFF, 0xFF]), 0x0000);
    assert_eq!(checksum(&[0x12, 0x34]), 0xEDCB);
}

#[test]
fn test_checksum_with_carry() {
    assert_eq!(checksum(&[0xFF, 0xFF, 0x00, 0x01]), 0xFFFE);
    assert_eq!(checksum(&[0xFF, 0xFF, 0xFF, 0xFF]), 0x0000);
}

#[test]
fn test_checksum_odd_length() {
    assert_eq!(checksum(&[0x01]), 0xFEFF);
    assert_eq!(checksum(&[0x00]), 0xFFFF);
    // words: 0x0102 + 0x0300 (odd byte in high position) = 0x0402 -> ~0x0402
    assert_eq!(checksum(&[0x01, 0x02, 0x03]), 0xFBFD);
}

#[test]
fn test_checksum_known_ip_header() {
    let hdr: [u8; 20] = [
        0x45, 0x00, 0x00, 0x54, 0x00, 0x00, 0x40, 0x00, 0x40, 0x01, 0x00, 0x00, 0xC0, 0xA8, 0x01,
        0x01, 0xC0, 0xA8, 0x01, 0x02,
    ];
    assert_eq!(checksum(&hdr), 0xB755);
}

#[test]
fn test_arp_cache_insert_lookup() {
    unsafe {
        let ip1 = [10, 8, 8, 1]; // unique range: cache is global across tests
        let mac1 = [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF];

        assert_eq!(arp_lookup(ip1), None);

        arp_insert(ip1, mac1);
        assert_eq!(arp_lookup(ip1), Some(mac1));

        let mac2 = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66];
        arp_insert(ip1, mac2);
        assert_eq!(arp_lookup(ip1), Some(mac2));

        let ip2 = [192, 168, 1, 1];
        assert_eq!(arp_lookup(ip2), None);

        arp_insert(ip2, mac1);
        assert_eq!(arp_lookup(ip2), Some(mac1));
        assert_eq!(arp_lookup(ip1), Some(mac2));
    }
}

#[test]
fn test_arp_cache_insert_many() {
    unsafe {
        // NOTE: the ARP cache is a process-global static; use IPs unique to this
        // test so parallel/order-dependent pollution from other tests can't leak.
        let ips = [[10, 7, 7, 1], [10, 7, 7, 2], [10, 7, 7, 3]];
        let macs = [
            [0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA],
            [0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB],
            [0xCC, 0xCC, 0xCC, 0xCC, 0xCC, 0xCC],
        ];

        for i in 0..3 {
            assert_eq!(arp_lookup(ips[i]), None);
            arp_insert(ips[i], macs[i]);
        }

        for i in 0..3 {
            assert_eq!(arp_lookup(ips[i]), Some(macs[i]));
        }
    }
}

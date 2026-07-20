use super::eth::{arp_insert, arp_lookup};
use super::ip::checksum;

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
    assert_eq!(checksum(&[0x01, 0x02, 0x03]), 0xFCFB);
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
        let ip1 = [10, 0, 0, 1];
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
        let ips = [[10, 0, 0, 1], [10, 0, 0, 2], [10, 0, 0, 3]];
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

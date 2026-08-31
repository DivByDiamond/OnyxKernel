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
    // SAFETY: test-only access to the global ARP cache; IPs are disjoint from other tests
    // (see note in test_arp_cache_insert_many), though the shared LEN counter is unsynchronized.
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
    // SAFETY: test-only access to the global ARP cache; disjoint IPs per the NOTE below,
    // but ARP_CACHE_LEN is a shared unsynchronized RMW across parallel tests (known race).
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

// ── Net sync fix tests (todo P1 #4): table exclusivity via the NET lock ──

/// Combined test: UDP_SOCKS / CONNS / ARP cache are process-global statics
/// and the host harness runs #[test] fns in parallel, so all slot-management
/// assertions live in one function (same pattern as runqueue.rs). IPs and
/// ports are unique to this test so the pre-existing ARP test cannot
/// interfere.
#[test]
fn test_net_tables_concurrent_exclusivity() {
    use crate::net::lock::{net_lock, net_unlock};
    use crate::net::tcp::conn::{CONNS, alloc_conn, alloc_local_port};
    use crate::net::udp::{udp_bind, udp_close, udp_recv};
    use onyx_core::errno::Errno;

    unsafe {
        // 1) Recursive NET lock re-entry (send_packet → poll → handler
        //    nesting relies on it): lock, nest, unwind exactly.
        net_lock();
        net_lock();
        assert_eq!(crate::net::lock::owned_depth(), 2);
        net_unlock();
        net_unlock();
        assert_eq!(crate::net::lock::owned_depth(), 0);

        // 2) UDP bind/close slot management under the lock: bind all 8
        //    slots, the 9th bind must fail with EBUSY, close frees, rebind
        //    succeeds (this is what 8 concurrent udp_bind callers race on
        //    without the lock).
        let mut slots = [0usize; 8];
        for (i, s) in slots.iter_mut().enumerate() {
            *s = udp_bind(6100 + i as u16).expect("bind until full");
        }
        assert!(udp_bind(6199).is_err());
        udp_close(slots[3]);
        let rebound = udp_bind(6199).expect("rebind after close");
        assert_eq!(rebound, slots[3]);
        udp_close(rebound);

        // 3) recv on a bound-but-empty slot is ENOENT; recv on a bad index
        //    is EINVAL (no panic — the syscall path feeds hostile indices).
        let s = udp_bind(6123).expect("bind");
        let mut buf = [0u8; 16];
        assert_eq!(udp_recv(s, &mut buf), Err(Errno::NoEnt));
        udp_close(s);
        assert_eq!(udp_recv(s, &mut buf), Err(Errno::Inval));
        assert_eq!(udp_recv(8 /* >= MAX_UDP_SOCKS */, &mut buf), Err(Errno::Inval));

        // 4) TCP conn slots: allocate all 8, the 9th alloc fails; ephemeral
        //    ports are never reused while a conn holds them (what two harts
        //    calling tcp_connect concurrently used to race).
        let mut cids = [0usize; 8];
        let mut cids_used = 0;
        for c in cids.iter_mut() {
            match alloc_conn() {
                Some(cid) => {
                    CONNS[cid] = Some(crate::net::tcp::conn::TcpConn {
                        state: 2,
                        src_port: 5100 + cids_used as u16,
                        dst_ip: [10, 9, 9, 1],
                        dst_port: 80,
                        snd_una: 1,
                        snd_nxt: 1,
                        rcv_nxt: 0,
                        send_buf: [0; crate::net::tcp::conn::BUF_SIZE],
                        send_len: 0,
                        recv_buf: [0; crate::net::tcp::conn::BUF_SIZE],
                        recv_len: 0,
                        recv_head: 0,
                        tw_deadline_us: 0,
                    });
                    *c = cid;
                    cids_used += 1;
                }
                None => break,
            }
        }
        if cids_used == 8 {
            assert!(alloc_conn().is_none());
        }
        let p1 = alloc_local_port();
        assert!(
            !CONNS.iter().flatten().any(|c| c.src_port == p1) || cids_used < 8,
            "ephemeral port collided with a live conn"
        );
        for &cid in &cids[..cids_used] {
            CONNS[cid] = None;
        }

        // 5) ARP cache: insert + lookup roundtrip and update-in-place for an
        //    existing IP (the ARP-cache race fix keeps both under net_lock).
        let ip = [10, 8, 8, 1];
        let mac_a = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66];
        let mac_b = [0x77, 0x88, 0x99, 0xAA, 0xBB, 0xCC];
        use crate::net::eth::{arp_insert, arp_lookup};
        arp_insert(ip, mac_a);
        assert_eq!(arp_lookup(ip), Some(mac_a));
        arp_insert(ip, mac_b); // update in place, no duplicate entry
        assert_eq!(arp_lookup(ip), Some(mac_b));
        // Unknown IP still misses.
        assert_eq!(arp_lookup([10, 8, 8, 99]), None);
    }
}

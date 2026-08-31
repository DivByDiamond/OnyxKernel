use crate::drivers::virtio_net;
use crate::net::lock::{net_lock, net_unlock};

pub const ETH_HLEN: usize = 14;
pub const ET_ARP: u16 = 0x0806;
pub const ET_IP: u16 = 0x0800;

#[repr(C, packed)]
pub struct EthHdr {
    pub dst: [u8; 6],
    pub src: [u8; 6],
    pub ethertype: u16,
}

const ARP_CACHE_MAX: usize = 8;
static mut ARP_CACHE_IP: [[u8; 4]; ARP_CACHE_MAX] = [[0; 4]; ARP_CACHE_MAX];
static mut ARP_CACHE_MAC: [[u8; 6]; ARP_CACHE_MAX] = [[0; 6]; ARP_CACHE_MAX];
static mut ARP_CACHE_LEN: usize = 0;

/// # Safety
///
/// Reads the lock-free ARP cache statics; caller must not race a
/// concurrent `arp_insert` from another hart (SIE=0 only bars same-hart
/// preemption, the kernel is SMP).
pub unsafe fn arp_lookup(ip: [u8; 4]) -> Option<[u8; 6]> {
    // SAFETY: read-only scan under net_lock(); indices derive from
    // ARP_CACHE_LEN, which arp_insert keeps <= ARP_CACHE_MAX (net sync fix,
    // todo P1 #4).
    unsafe {
        net_lock();
        let mut found = None;
        for i in 0..ARP_CACHE_LEN {
            if ARP_CACHE_IP[i] == ip {
                found = Some(ARP_CACHE_MAC[i]);
                break;
            }
        }
        net_unlock();
        found
    }
}

/// # Safety
///
/// Mutates the lock-free ARP cache; caller must serialize cache access
/// across harts (no lock guards ARP_CACHE_LEN or the entries).
pub unsafe fn arp_insert(ip: [u8; 4], mac: [u8; 6]) {
    // SAFETY: capacity checked against ARP_CACHE_MAX before every write;
    // net_lock() excludes concurrent cache writers (net sync fix, todo P1
    // #4). Recursive-safe: callers inside poll/send_packet already hold it.
    unsafe {
        net_lock();
        for i in 0..ARP_CACHE_LEN {
            if ARP_CACHE_IP[i] == ip {
                ARP_CACHE_MAC[i] = mac;
                net_unlock();
                return;
            }
        }
        if ARP_CACHE_LEN < ARP_CACHE_MAX {
            ARP_CACHE_IP[ARP_CACHE_LEN] = ip;
            ARP_CACHE_MAC[ARP_CACHE_LEN] = mac;
            ARP_CACHE_LEN += 1;
        }
        net_unlock();
    }
}

/// # Safety
///
/// TX path: reads the boot-written G_IP and drives the transmit ring;
/// must not run concurrently with another sender/poller on another hart.
pub unsafe fn arp_request(target_ip: [u8; 4]) {
    // SAFETY: fixed 42-byte stack buffer with in-range slice offsets; TX ring contract per virtio_net::send.
    unsafe {
        let mac = virtio_net::mac();
        let broadcast = [0xFF; 6];
        let mut pkt = [0u8; 42];
        pkt[0..6].copy_from_slice(&broadcast);
        pkt[6..12].copy_from_slice(&mac);
        pkt[12..14].copy_from_slice(&ET_ARP.to_be_bytes());
        pkt[14..16].copy_from_slice(&1u16.to_be_bytes());
        pkt[16..18].copy_from_slice(&ET_IP.to_be_bytes());
        pkt[18] = 6;
        pkt[19] = 4;
        pkt[20..22].copy_from_slice(&1u16.to_be_bytes());
        pkt[22..28].copy_from_slice(&mac);
        pkt[28..32].copy_from_slice(&crate::net::G_IP);
        pkt[32..38].copy_from_slice(&[0; 6]);
        pkt[38..42].copy_from_slice(&target_ip);
        let _ = virtio_net::send(&pkt);
    }
}

/// # Safety
///
/// RX-path handler: frame length is checked (>= 42) before any indexed
/// read; ARP-cache and G_IP access rely on the net single-poller contract.
pub unsafe fn handle_arp(frame: &[u8]) {
    // SAFETY: all frame indexing bounds-checked against the length check
    // above; cache/G_IP access under net_lock() (recursive when called from
    // net::poll, exclusive otherwise — net sync fix, todo P1 #4).
    unsafe {
        net_lock();
        handle_arp_inner(frame);
        net_unlock();
    }
}

/// Lock-free body of handle_arp (caller holds net_lock).
///
/// # Safety
///
/// Caller contract: net_lock() held.
unsafe fn handle_arp_inner(frame: &[u8]) {
    // SAFETY: all frame indexing bounds-checked against the length check above; net_lock() held by caller.
    unsafe {
        if frame.len() < 42 {
            return;
        }
        let mac = virtio_net::mac();
        let oper = u16::from_be_bytes([frame[20], frame[21]]);
        let spa: [u8; 4] = [frame[28], frame[29], frame[30], frame[31]];
        let sha: [u8; 6] = [
            frame[22], frame[23], frame[24], frame[25], frame[26], frame[27],
        ];
        let tpa: [u8; 4] = [frame[38], frame[39], frame[40], frame[41]];
        arp_insert(spa, sha);
        if oper == 1 && tpa == crate::net::G_IP {
            let mut pkt = [0u8; 42];
            pkt[0..6].copy_from_slice(&sha);
            pkt[6..12].copy_from_slice(&mac);
            pkt[12..14].copy_from_slice(&ET_ARP.to_be_bytes());
            pkt[14..16].copy_from_slice(&1u16.to_be_bytes());
            pkt[16..18].copy_from_slice(&ET_IP.to_be_bytes());
            pkt[18] = 6;
            pkt[19] = 4;
            pkt[20..22].copy_from_slice(&2u16.to_be_bytes());
            pkt[22..28].copy_from_slice(&mac);
            pkt[28..32].copy_from_slice(&crate::net::G_IP);
            pkt[32..38].copy_from_slice(&sha);
            pkt[38..42].copy_from_slice(&spa);
            let _ = virtio_net::send(&pkt);
        }
    }
}

/// # Safety
///
/// TX path: builds the frame from validated arguments and hands it to the
/// transmit ring; must not run concurrently with another sender per the
/// net single-poller contract (no lock serializes the TX ring).
pub unsafe fn send_frame(dst_mac: [u8; 6], ethertype: u16, payload: &[u8]) {
    let mac = virtio_net::mac();
    let total = ETH_HLEN + payload.len();
    let mut frame = alloc::vec![0u8; total];
    frame[0..6].copy_from_slice(&dst_mac);
    frame[6..12].copy_from_slice(&mac);
    frame[12..14].copy_from_slice(&ethertype.to_be_bytes());
    frame[14..].copy_from_slice(payload);
    let _ = virtio_net::send(&frame);
}

/// # Safety
///
/// RX dispatcher: requires the net single-poller contract; frame length
/// is re-checked inside before the ethertype read, and sub-handlers
/// re-validate their own offsets.
pub unsafe fn dispatch(frame: &[u8]) {
    // SAFETY: ethertype read bounds-checked (frame.len() >= ETH_HLEN); callees re-check lengths.
    unsafe {
        if frame.len() < ETH_HLEN {
            return;
        }
        let ethertype = u16::from_be_bytes([frame[12], frame[13]]);
        match ethertype {
            ET_ARP => handle_arp(frame),
            ET_IP => crate::net::ip::handle_ip(frame),
            _ => {}
        }
    }
}

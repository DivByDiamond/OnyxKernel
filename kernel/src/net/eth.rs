use crate::drivers::virtio_net;
use core::ptr;

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

pub unsafe fn arp_lookup(ip: [u8; 4]) -> Option<[u8; 6]> {
    for i in 0..ARP_CACHE_LEN {
        if ARP_CACHE_IP[i] == ip {
            return Some(ARP_CACHE_MAC[i]);
        }
    }
    None
}

pub unsafe fn arp_insert(ip: [u8; 4], mac: [u8; 6]) {
    for i in 0..ARP_CACHE_LEN {
        if ARP_CACHE_IP[i] == ip {
            ARP_CACHE_MAC[i] = mac;
            return;
        }
    }
    if ARP_CACHE_LEN < ARP_CACHE_MAX {
        ARP_CACHE_IP[ARP_CACHE_LEN] = ip;
        ARP_CACHE_MAC[ARP_CACHE_LEN] = mac;
        ARP_CACHE_LEN += 1;
    }
}

pub unsafe fn arp_request(target_ip: [u8; 4]) {
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

pub unsafe fn handle_arp(frame: &[u8]) {
    if frame.len() < 42 { return; }
    let mac = virtio_net::mac();
    let oper = u16::from_be_bytes([frame[20], frame[21]]);
    let spa: [u8; 4] = [frame[28], frame[29], frame[30], frame[31]];
    let sha: [u8; 6] = [frame[22], frame[23], frame[24], frame[25], frame[26], frame[27]];
    let tpa: [u8; 4] = [frame[38], frame[39], frame[40], frame[41]];
    arp_insert(spa, sha);
    if oper == 1 && tpa == crate::net::G_IP {
        let mut pkt = [0u8; 42];
        pkt[0..6].copy_from_slice(&sha);
        pkt[6..12].copy_from_slice(&mac);
        pkt[12..14].copy_from_slice(&ET_ARP.to_be_bytes());
        pkt[14..16].copy_from_slice(&1u16.to_be_bytes());
        pkt[16..18].copy_from_slice(&ET_IP.to_be_bytes());
        pkt[18] = 6; pkt[19] = 4;
        pkt[20..22].copy_from_slice(&2u16.to_be_bytes());
        pkt[22..28].copy_from_slice(&mac);
        pkt[28..32].copy_from_slice(&crate::net::G_IP);
        pkt[32..38].copy_from_slice(&sha);
        pkt[38..42].copy_from_slice(&spa);
        let _ = virtio_net::send(&pkt);
    }
}

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

pub unsafe fn dispatch(frame: &[u8]) {
    if frame.len() < ETH_HLEN { return; }
    let ethertype = u16::from_be_bytes([frame[12], frame[13]]);
    match ethertype {
        ET_ARP => handle_arp(frame),
        ET_IP => crate::net::ip::handle_ip(frame),
        _ => {}
    }
}

use crate::drivers::virtio_net::NET_MTU;
use crate::net::G_IP;
use crate::net::eth;
use onyx_core::errno::{Errno, KResult};

pub const IP_HLEN: usize = 20;
pub const IP_PROTO_TCP: u8 = 6;
pub const IP_PROTO_ICMP: u8 = 1;
pub const IP_PROTO_UDP: u8 = 17;

static mut IP_ID: u16 = 0;

pub fn checksum(data: &[u8]) -> u16 {
    let mut sum = 0u32;
    let mut i = 0;
    while i + 1 < data.len() {
        sum = sum.wrapping_add(u16::from_be_bytes([data[i], data[i + 1]]) as u32);
        i += 2;
    }
    if i < data.len() {
        sum = sum.wrapping_add((data[i] as u32) << 8);
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16)
}

pub unsafe fn send_packet(dst_ip: [u8; 4], protocol: u8, payload: &[u8]) -> KResult<()> {
    let dst_mac = if dst_ip == [255, 255, 255, 255] {
        [0xFF; 6]
    } else {
        match eth::arp_lookup(dst_ip) {
            Some(m) => m,
            None => {
                eth::arp_request(dst_ip);
                let mut found = None;
                for _ in 0..1000 {
                    crate::net::poll();
                    if let Some(m) = eth::arp_lookup(dst_ip) {
                        found = Some(m);
                        break;
                    }
                }
                match found {
                    Some(m) => m,
                    None => return Err(Errno::Io),
                }
            }
        }
    };

    let total_len = IP_HLEN + payload.len();
    let mut pkt = alloc::vec![0u8; total_len];
    let id = {
        let id = IP_ID;
        IP_ID = id.wrapping_add(1);
        id
    };
    pkt[0] = 0x45;
    pkt[1] = 0;
    pkt[2..4].copy_from_slice(&(total_len as u16).to_be_bytes());
    pkt[4..6].copy_from_slice(&id.to_be_bytes());
    pkt[6..8].copy_from_slice(&0u16.to_be_bytes());
    pkt[8] = 64;
    pkt[9] = protocol;
    pkt[10..12].copy_from_slice(&[0, 0]);
    pkt[12..16].copy_from_slice(&G_IP);
    pkt[16..20].copy_from_slice(&dst_ip);
    if !payload.is_empty() {
        pkt[20..].copy_from_slice(payload);
    }
    let ck = checksum(&pkt[..IP_HLEN]);
    pkt[10..12].copy_from_slice(&ck.to_be_bytes());
    eth::send_frame(dst_mac, eth::ET_IP, &pkt);
    Ok(())
}

pub unsafe fn handle_ip(frame: &[u8]) {
    if frame.len() < eth::ETH_HLEN + IP_HLEN {
        return;
    }
    let ip_start = eth::ETH_HLEN;
    let ihl = (frame[ip_start] & 0x0F) as usize * 4;
    let total_len = u16::from_be_bytes([frame[ip_start + 2], frame[ip_start + 3]]) as usize;
    let protocol = frame[ip_start + 9];
    match protocol {
        IP_PROTO_ICMP => unsafe { handle_icmp(frame, ip_start, ihl, total_len) },
        IP_PROTO_TCP => unsafe { crate::net::tcp::handle_tcp(frame, ip_start, ihl, total_len) },
        IP_PROTO_UDP => unsafe { crate::net::udp::handle_udp(frame, ip_start) },
        _ => {}
    }
}

unsafe fn handle_icmp(frame: &[u8], ip_start: usize, _ihl: usize, _total_len: usize) {
    let icmp_start = ip_start + IP_HLEN;
    if frame.len() < icmp_start + 8 {
        return;
    }
    if frame[icmp_start] != 8 {
        return;
    }
    let mut reply = alloc::vec![0u8; frame.len() - ip_start];
    reply[0] = 0;
    reply[1] = 0;
    reply[2..4].copy_from_slice(&[0, 0]);
    if reply.len() > 4 {
        reply[4..].copy_from_slice(&frame[icmp_start + 4..]);
    }
    let ck = checksum(&reply);
    reply[2..4].copy_from_slice(&ck.to_be_bytes());
    let mut ip_pkt = alloc::vec![0u8; IP_HLEN + reply.len()];
    ip_pkt[0] = 0x45;
    let total = IP_HLEN + reply.len();
    ip_pkt[2..4].copy_from_slice(&(total as u16).to_be_bytes());
    ip_pkt[8] = 64;
    ip_pkt[9] = IP_PROTO_ICMP;
    let src: [u8; 4] = [
        frame[ip_start + 16],
        frame[ip_start + 17],
        frame[ip_start + 18],
        frame[ip_start + 19],
    ];
    ip_pkt[12..16].copy_from_slice(&src);
    ip_pkt[16..20].copy_from_slice(&G_IP);
    let ck = checksum(&ip_pkt[..IP_HLEN]);
    ip_pkt[10..12].copy_from_slice(&ck.to_be_bytes());
    ip_pkt[20..].copy_from_slice(&reply);
    let src_mac = [frame[6], frame[7], frame[8], frame[9], frame[10], frame[11]];
    eth::send_frame(src_mac, eth::ET_IP, &ip_pkt);
}

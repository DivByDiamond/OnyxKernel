use crate::net::G_IP;
use crate::net::eth;
use crate::net::lock::{net_lock, net_unlock};
use onyx_core::errno::{Errno, KResult};

pub const IP_HLEN: usize = 20;
pub const IP_PROTO_TCP: u8 = 6;
pub const IP_PROTO_ICMP: u8 = 1;
pub const IP_PROTO_UDP: u8 = 17;

static mut IP_ID: u16 = 0;

pub use super::checksum::checksum;

/// # Safety
///
/// TX path: reads G_IP, RMWs IP_ID and drives the rings; must not run
/// concurrently with another sender/poller on a different hart (IP_ID
/// and the ARP cache are unguarded; SIE=0 only bars same-hart preemption).
pub unsafe fn send_packet(dst_ip: [u8; 4], protocol: u8, payload: &[u8]) -> KResult<()> {
    // SAFETY: header vector sized IP_HLEN + payload.len().
    //
    // Net sync fix (todo P1 #4) + reentrancy fix (todo P1 #6): the whole
    // TX path runs under the recursive NET lock, which makes the IP_ID RMW
    // and the ARP cache lookups atomic w.r.t. other harts AND makes the
    // poll() call below (ARP-resolve wait loop) safe on this hart — poll's
    // dispatch handlers re-acquire the same lock recursively (owner-only).
    // The old "single-sender contract" is enforced by the lock now.
    unsafe {
        net_lock();
        let result = send_packet_inner(dst_ip, protocol, payload);
        net_unlock();
        result
    }
}

/// Lock-free body of send_packet (caller holds net_lock).
///
/// # Safety
///
/// Caller contract: net_lock() held; may re-enter poll() (recursive lock).
unsafe fn send_packet_inner(dst_ip: [u8; 4], protocol: u8, payload: &[u8]) -> KResult<()> {
    // SAFETY: header vector sized IP_HLEN + payload.len(); IP_ID RMW and ARP cache accessed under the caller's net_lock().
    unsafe {
        if payload.len() > 1480 { return Err(Errno::Range); }
        let mask = u32::from_be_bytes(crate::net::G_MASK);
        let local = u32::from_be_bytes(G_IP);
        let destination = u32::from_be_bytes(dst_ip);
        let broadcast = dst_ip == [255;4] || (mask != 0 && destination == (local | !mask));
        let next_hop = if !broadcast && (destination & mask) != (local & mask) {
            if crate::net::G_GW == [0;4] { return Err(Errno::Io); }
            crate::net::G_GW
        } else { dst_ip };
        let dst_mac = if broadcast {
            [0xFF; 6]
        } else {
            match eth::arp_lookup(next_hop) {
                Some(m) => m,
                None => {
                    eth::arp_request(next_hop);
                    let mut found = None;
                    for _ in 0..1000 {
                        crate::net::poll();
                        if let Some(m) = eth::arp_lookup(next_hop) {
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
}

/// # Safety
///
/// RX dispatcher: requires the net single-poller contract; protocol
/// dispatches to sub-handlers that validate their own offsets.
pub unsafe fn handle_ip(frame: &[u8]) {
    let Some(packet) = super::ipv4::parse(frame) else { return; };
    // SAFETY: context selected under net_lock; validated sizes bound every slice.
    unsafe {
        let mask = u32::from_be_bytes(crate::net::G_MASK);
        let broadcast = packet.destination == [255;4] || (mask != 0 && u32::from_be_bytes(packet.destination) == (u32::from_be_bytes(G_IP) | !mask));
        if packet.destination != G_IP && !broadcast { return; }
        let frame = &frame[..eth::ETH_HLEN + packet.total_len];
        match packet.protocol {
            IP_PROTO_ICMP if !broadcast => {
                let payload = &frame[eth::ETH_HLEN + packet.header_len..];
                if let Some(reply) = super::ipv4::echo_reply(payload) {
                    let _ = send_packet(packet.source, IP_PROTO_ICMP, &reply);
                }
            },
            IP_PROTO_TCP if !broadcast => crate::net::tcp::handle_tcp(frame, eth::ETH_HLEN, packet.header_len, packet.total_len),
            IP_PROTO_UDP => crate::net::udp::handle_udp(frame, eth::ETH_HLEN),
            _ => {},
        }
    }
}

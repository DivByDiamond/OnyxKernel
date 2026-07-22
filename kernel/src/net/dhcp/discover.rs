use crate::drivers::virtio_net;
use crate::net::poll;
use crate::net::udp;
use onyx_core::errno::{Errno, KResult};

use super::protocol::{self, DHCP_CLIENT_PORT, DHCP_SERVER_PORT};

pub unsafe fn dhcp_discover() -> KResult<([u8; 4], [u8; 4], [u8; 4], [u8; 4])> {
    let mac = virtio_net::mac();
    let xid = crate::srv::timer::uptime_us() as u32;
    let sock = udp::udp_bind(DHCP_CLIENT_PORT)?;
    let discover = protocol::make_dhcp_msg(1, xid, &mac, None, None);
    udp::udp_sendto([255, 255, 255, 255], DHCP_SERVER_PORT, &discover)?;
    let mut offered_ip = [0u8; 4];
    let mut server_id = [0u8; 4];
    let mut subnet_mask = [0u8; 4];
    let mut dns_server = [0u8; 4];
    let mut got_offer = false;
    for _ in 0..50000 {
        poll();
        let mut buf = [0u8; 2048];
        if let Ok(n) = udp::udp_recv(sock, &mut buf) {
            if let Some((msg_type, yiaddr, sid, mask, dns)) = protocol::parse_dhcp_reply(&buf[..n])
            {
                if msg_type == 2 && !yiaddr.iter().all(|&b| b == 0) {
                    offered_ip = yiaddr;
                    server_id = sid;
                    subnet_mask = mask;
                    dns_server = dns;
                    got_offer = true;
                    break;
                }
            }
        }
    }
    if !got_offer {
        udp::udp_close(sock);
        return Err(Errno::Io);
    }
    let request = protocol::make_dhcp_msg(3, xid, &mac, Some(offered_ip), Some(server_id));
    udp::udp_sendto([255, 255, 255, 255], DHCP_SERVER_PORT, &request)?;
    let mut got_ack = false;
    for _ in 0..50000 {
        poll();
        let mut buf = [0u8; 2048];
        if let Ok(n) = udp::udp_recv(sock, &mut buf) {
            if let Some((msg_type, yiaddr, _sid, mask, dns)) = protocol::parse_dhcp_reply(&buf[..n])
            {
                if msg_type == 5 {
                    if !yiaddr.iter().all(|&b| b == 0) {
                        offered_ip = yiaddr;
                    }
                    if !mask.iter().all(|&b| b == 0) {
                        subnet_mask = mask;
                    }
                    if !dns.iter().all(|&b| b == 0) {
                        dns_server = dns;
                    }
                    got_ack = true;
                    break;
                }
            }
        }
    }
    udp::udp_close(sock);
    if !got_ack {
        return Err(Errno::Io);
    }
    let gateway = if !subnet_mask.iter().all(|&b| b == 0) {
        let gw = [offered_ip[0], offered_ip[1], offered_ip[2], 1];
        if gw != offered_ip {
            gw
        } else {
            [offered_ip[0], offered_ip[1], offered_ip[2], 254]
        }
    } else {
        [offered_ip[0], offered_ip[1], offered_ip[2], 1]
    };
    let final_mask = if subnet_mask.iter().all(|&b| b == 0) {
        [255, 255, 255, 0]
    } else {
        subnet_mask
    };
    Ok((offered_ip, final_mask, gateway, dns_server))
}

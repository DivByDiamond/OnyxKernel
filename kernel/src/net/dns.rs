use crate::net::poll;
use crate::net::udp;
use onyx_core::errno::{Errno, KResult};

const DNS_PORT: u16 = 53;

fn dns_encode_name(name: &[u8]) -> alloc::vec::Vec<u8> {
    let mut result = alloc::vec![];
    for part in name.split(|&b| b == b'.') {
        if !part.is_empty() {
            result.push(part.len() as u8);
            result.extend_from_slice(part);
        }
    }
    result.push(0);
    result
}

fn dns_skip_name(msg: &[u8], mut off: usize) -> Option<usize> {
    loop {
        if off >= msg.len() {
            return None;
        }
        let b = msg[off];
        if b == 0 {
            return Some(off + 1);
        }
        if b & 0xC0 == 0xC0 {
            return Some(off + 2);
        }
        off += 1 + b as usize;
    }
}

pub unsafe fn dns_resolve(hostname: &[u8], dns_server: [u8; 4]) -> KResult<[u8; 4]> {
    let encoded = dns_encode_name(hostname);
    let qlen = encoded.len() + 4;
    let mut query = alloc::vec![0u8; 12 + qlen];
    let id = (crate::srv::timer::uptime_us() & 0xFFFF) as u16;
    query[0..2].copy_from_slice(&id.to_be_bytes());
    query[2..4].copy_from_slice(&0x0100u16.to_be_bytes());
    query[4..6].copy_from_slice(&1u16.to_be_bytes());
    query[6..12].copy_from_slice(&[0; 6]);
    query[12..12 + encoded.len()].copy_from_slice(&encoded);
    let qoff = 12 + encoded.len();
    query[qoff..qoff + 2].copy_from_slice(&1u16.to_be_bytes());
    query[qoff + 2..qoff + 4].copy_from_slice(&1u16.to_be_bytes());
    let sock = udp::udp_bind(0)?;
    udp::udp_sendto(dns_server, DNS_PORT, &query)?;
    for _ in 0..30000 {
        poll();
        let mut buf = [0u8; 512];
        if let Ok(n) = udp::udp_recv(sock, &mut buf) {
            if n < 12 {
                continue;
            }
            let rid = u16::from_be_bytes([buf[0], buf[1]]);
            if rid != id {
                continue;
            }
            let flags = u16::from_be_bytes([buf[2], buf[3]]);
            if flags & 0x8000 == 0 {
                continue;
            }
            if flags & 0x000F != 0 {
                continue;
            }
            let qdcount = u16::from_be_bytes([buf[4], buf[5]]);
            let ancount = u16::from_be_bytes([buf[6], buf[7]]);
            if ancount == 0 {
                continue;
            }
            let mut off = 12usize;
            for _ in 0..qdcount {
                off = match dns_skip_name(&buf, off) {
                    Some(o) => o,
                    None => break,
                };
                off += 4;
            }
            for _ in 0..ancount {
                off = match dns_skip_name(&buf, off) {
                    Some(o) => o,
                    None => break,
                };
                if off + 10 > n {
                    break;
                }
                let atype = u16::from_be_bytes([buf[off], buf[off + 1]]);
                let aclass = u16::from_be_bytes([buf[off + 2], buf[off + 3]]);
                let rdlength = u16::from_be_bytes([buf[off + 8], buf[off + 9]]) as usize;
                off += 10;
                if atype == 1 && aclass == 1 && rdlength == 4 && off + 4 <= n {
                    let ip = [buf[off], buf[off + 1], buf[off + 2], buf[off + 3]];
                    udp::udp_close(sock);
                    return Ok(ip);
                }
                off += rdlength;
            }
        }
    }
    udp::udp_close(sock);
    Err(Errno::Io)
}

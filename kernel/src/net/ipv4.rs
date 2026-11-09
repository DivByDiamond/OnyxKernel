//! Bounds/checksum validation shared by IPv4 protocol dispatch and host tests.
use super::{checksum::checksum, eth::ETH_HLEN};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Packet {
    pub header_len: usize,
    pub total_len: usize,
    pub protocol: u8,
    pub source: [u8; 4],
    pub destination: [u8; 4],
}

pub fn parse(frame: &[u8]) -> Option<Packet> {
    let ip = frame.get(ETH_HLEN..)?;
    if ip.len() < 20 || ip[0] >> 4 != 4 || ip[8] == 0 {
        return None;
    }
    let header_len = usize::from(ip[0] & 15) * 4;
    let total_len = usize::from(u16::from_be_bytes([ip[2], ip[3]]));
    if header_len < 20 || total_len < header_len || total_len > ip.len() {
        return None;
    }
    if checksum(ip.get(..header_len)?) != 0 {
        return None;
    }
    // Fragment reassembly is deliberately unsupported: reject MF or a nonzero offset.
    if u16::from_be_bytes([ip[6], ip[7]]) & 0x3fff != 0 {
        return None;
    }
    Some(Packet {
        header_len,
        total_len,
        protocol: ip[9],
        source: ip[12..16].try_into().ok()?,
        destination: ip[16..20].try_into().ok()?,
    })
}

pub fn echo_reply(request: &[u8]) -> Option<alloc::vec::Vec<u8>> {
    if request.len() < 8 || request[0] != 8 || request[1] != 0 || checksum(request) != 0 {
        return None;
    }
    let mut reply = request.to_vec();
    reply[0] = 0;
    reply[2..4].fill(0);
    let sum = checksum(&reply);
    reply[2..4].copy_from_slice(&sum.to_be_bytes());
    Some(reply)
}

#[cfg(test)]
mod tests;

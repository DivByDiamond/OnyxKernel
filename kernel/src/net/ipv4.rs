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
    if ip.len() < 20 || ip[0] >> 4 != 4 || ip[8] == 0 { return None; }
    let header_len = usize::from(ip[0] & 15) * 4;
    let total_len = usize::from(u16::from_be_bytes([ip[2], ip[3]]));
    if header_len < 20 || total_len < header_len || total_len > ip.len() { return None; }
    if checksum(ip.get(..header_len)?) != 0 { return None; }
    // Fragment reassembly is deliberately unsupported: reject MF or a nonzero offset.
    if u16::from_be_bytes([ip[6], ip[7]]) & 0x3fff != 0 { return None; }
    Some(Packet {
        header_len, total_len, protocol: ip[9],
        source: ip[12..16].try_into().ok()?, destination: ip[16..20].try_into().ok()?,
    })
}

pub fn echo_reply(request: &[u8]) -> Option<alloc::vec::Vec<u8>> {
    if request.len() < 8 || request[0] != 8 || request[1] != 0 || checksum(request) != 0 { return None; }
    let mut reply = request.to_vec();
    reply[0] = 0;
    reply[2..4].fill(0);
    let sum = checksum(&reply);
    reply[2..4].copy_from_slice(&sum.to_be_bytes());
    Some(reply)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn packet() -> [u8; 42] {
        let mut frame = [0u8; 42];
        let ip = &mut frame[ETH_HLEN..];
        ip[0] = 0x45; ip[2..4].copy_from_slice(&28u16.to_be_bytes()); ip[8] = 64; ip[9] = 1;
        let sum = checksum(&ip[..20]); ip[10..12].copy_from_slice(&sum.to_be_bytes());
        frame
    }
    #[test]
    fn rejects_truncation_bad_header_checksum_and_fragments() {
        let _serial = crate::net::lock::test_serial();
        let frame = packet();
        assert!(parse(&frame).is_some());
        assert!(parse(&frame[..41]).is_none());
        let mut bad = frame; bad[ETH_HLEN+8] ^= 1; assert!(parse(&bad).is_none());
        let mut fragment = frame; fragment[ETH_HLEN+6] = 0x20;
        fragment[ETH_HLEN+10..ETH_HLEN+12].fill(0);
        let sum = checksum(&fragment[ETH_HLEN..ETH_HLEN+20]);
        fragment[ETH_HLEN+10..ETH_HLEN+12].copy_from_slice(&sum.to_be_bytes());
        assert!(parse(&fragment).is_none());
    }
    #[test]
    fn echo_reply_preserves_exact_payload_and_reverses_type() {
        let _serial = crate::net::lock::test_serial();
        let mut request = alloc::vec![8,0,0,0,0x12,0x34,0,1,b'p',b'i',b'n',b'g'];
        let sum = checksum(&request); request[2..4].copy_from_slice(&sum.to_be_bytes());
        let reply = echo_reply(&request).expect("valid echo");
        assert_eq!(reply.len(), request.len());
        assert_eq!(reply[0],0); assert_eq!(&reply[4..],&request[4..]); assert_eq!(checksum(&reply),0);
        request[8] ^= 1; assert!(echo_reply(&request).is_none());
    }
}

//! Shared Internet checksum math (RFC 768 / RFC 793).
//!
//! `checksum()` covers the plain IPv4 header form; `pseudo_checksum()`
//! adds the UDP/TCP pseudo-header so both transport checksums share one
//! fold/complement implementation.

/// Sum of 16-bit big-endian words over `data` (odd length: zero-padded),
/// without folding or complementing. `wrapping_add` mirrors the historic
/// per-word overflow behaviour of the stack.
fn raw_sum(data: &[u8]) -> u32 {
    let mut sum = 0u32;
    let mut i = 0;
    while i + 1 < data.len() {
        sum = sum.wrapping_add(u16::from_be_bytes([data[i], data[i + 1]]) as u32);
        i += 2;
    }
    if i < data.len() {
        sum = sum.wrapping_add((data[i] as u32) << 8);
    }
    sum
}

fn fold(mut sum: u32) -> u16 {
    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16)
}

/// Plain one's-complement checksum (IPv4 header form).
pub fn checksum(data: &[u8]) -> u16 {
    fold(raw_sum(data))
}

/// UDP/TCP checksum: pseudo-header (src/dst IPv4, protocol, length =
/// segment.len()) followed by `segment`. On TX the result goes into the
/// checksum field; on RX feeding the whole segment *including* the field
/// must yield 0 for the segment to be valid.
pub fn pseudo_checksum(src_ip: &[u8; 4], dst_ip: &[u8; 4], proto: u8, segment: &[u8]) -> u16 {
    let mut sum = 0u32;
    for pair in [src_ip, dst_ip] {
        for i in 0..2 {
            sum = sum.wrapping_add(u16::from_be_bytes([pair[i * 2], pair[i * 2 + 1]]) as u32);
        }
    }
    sum = sum.wrapping_add(proto as u32);
    sum = sum.wrapping_add(segment.len() as u32);
    fold(sum.wrapping_add(raw_sum(segment)))
}

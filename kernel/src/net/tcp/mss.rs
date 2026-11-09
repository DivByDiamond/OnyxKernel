//! TCP Maximum Segment Size: option encoding/parsing and send-side limits.
//!
//! Both directions of this stack used to hard-code 536 bytes per segment —
//! the RFC 1122 fallback for peers that announce no MSS option. We now
//! announce our own MSS in SYN/SYN-ACK and parse the peer's, so a normal
//! path MTU of 1500 yields 1460-byte segments in both directions. There is
//! still no PMTU discovery: a path with MTU < 1500 will have our segments
//! fragmented in transit, and inbound fragments are dropped by `ipv4.rs`.
use super::conn::TCP_HLEN;
use super::state::SYN;

/// Maximum payload bytes per outbound segment: 1500 (IPv4 MTU) − 20 (IP)
/// − 20 (TCP). Exactly at `ip::send_packet`'s 1480-byte payload ceiling
/// and `virtio_net`'s 1514-byte frame ceiling.
pub(in crate::net) const TCP_MSS: usize = 1460;

/// RFC 1122 default when the peer's SYN carries no MSS option (or the
/// option is malformed). Also the value used before the handshake parses
/// one.
pub(in crate::net) const TCP_DEFAULT_MSS: u16 = 536;

/// Kind + length + 2-byte value of the MSS option.
pub(super) const MSS_OPT_LEN: usize = 4;

/// TCP header length for a segment with `flags`: SYN segments carry the
/// 4-byte MSS option (data offset 6); everything else is the bare
/// 20-byte header (data offset 5).
pub(in crate::net) const fn tcp_hdr_len(flags: u8) -> usize {
    if flags & SYN != 0 {
        TCP_HLEN + MSS_OPT_LEN
    } else {
        TCP_HLEN
    }
}

/// Wire encoding of our MSS option (kind=2, len=4, big-endian value).
pub(in crate::net) const fn mss_option(mss: u16) -> [u8; MSS_OPT_LEN] {
    [2, 4, (mss >> 8) as u8, (mss & 0xff) as u8]
}

/// Extract the peer's announced MSS from a TCP options slice (only
/// meaningful in SYN/SYN-ACK). Scanned with strict length checks: EOL/NOP
/// are walked, unknown options are skipped by their length byte, and any
/// truncation or a malformed MSS option yields `None` so the caller keeps
/// [`TCP_DEFAULT_MSS`] instead of trusting garbage.
pub(in crate::net) fn parse_mss_option(options: &[u8]) -> Option<u16> {
    let mut i = 0;
    while i < options.len() {
        match options[i] {
            0 => return None, // EOL: nothing valid may follow.
            1 => i += 1,      // NOP padding.
            2 => {
                // MSS: kind, len=4, then two value bytes — all in bounds.
                if i + MSS_OPT_LEN > options.len() || options[i + 1] != MSS_OPT_LEN as u8 {
                    return None;
                }
                return Some(u16::from_be_bytes([options[i + 2], options[i + 3]]));
            }
            _ => {
                // Generic option: length byte must exist, be at least 2
                // (kind + len) and stay inside the slice.
                if i + 1 >= options.len() {
                    return None;
                }
                let len = options[i + 1] as usize;
                if len < 2 || i + len > options.len() {
                    return None;
                }
                i += len;
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::super::state::{ACK, FIN};
    use super::*;

    #[test]
    fn syn_segments_are_four_bytes_longer_than_plain_ones() {
        assert_eq!(tcp_hdr_len(SYN), 24);
        assert_eq!(tcp_hdr_len(SYN | ACK), 24);
        assert_eq!(tcp_hdr_len(ACK), 20);
        assert_eq!(tcp_hdr_len(0x18), 20); // PSH|ACK data segment
        assert_eq!(tcp_hdr_len(FIN | ACK), 20);
    }

    #[test]
    fn mss_option_round_trips_through_the_parser() {
        for mss in [536u16, 1460, 1400, 1] {
            assert_eq!(parse_mss_option(&mss_option(mss)), Some(mss));
        }
        assert_eq!(mss_option(1460), [2, 4, 0x05, 0xb4]);
    }

    #[test]
    fn parser_walks_padding_and_skips_unknown_options() {
        // NOP NOP then MSS.
        let with_nops = [1u8, 1, 2, 4, 0x05, 0xb4];
        assert_eq!(parse_mss_option(&with_nops), Some(1460));
        // A generic option (kind=3, len=3: window scale) before the MSS.
        let with_wscale = [3u8, 3, 7, 2, 4, 0x02, 0x00];
        assert_eq!(parse_mss_option(&with_wscale), Some(512));
        // EOL terminates the scan: MSS-looking bytes after it are ignored.
        let eol_first = [0u8, 2, 4, 0x05, 0xb4];
        assert_eq!(parse_mss_option(&eol_first), None);
    }

    #[test]
    fn malformed_or_absent_options_yield_none() {
        assert_eq!(parse_mss_option(&[]), None);
        assert_eq!(parse_mss_option(&[1, 1]), None); // only padding
        assert_eq!(parse_mss_option(&[2, 4, 0x05]), None); // truncated value
        assert_eq!(parse_mss_option(&[2, 3, 0x05, 0xb4]), None); // bad len
        assert_eq!(parse_mss_option(&[9]), None); // kind without len byte
        assert_eq!(parse_mss_option(&[3, 40, 1, 2]), None); // len overruns
        assert_eq!(parse_mss_option(&[5, 2]), None); // kind 5, len 2, no MSS
    }
}

use crate::net::tcp::conn::state::{ACK, FIN};
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

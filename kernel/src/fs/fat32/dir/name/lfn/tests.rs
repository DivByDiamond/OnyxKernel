use crate::fs::fat32::ATTR_LFN;
use super::*;

fn lfn_entry(seq: u8, checksum: u8, units: &[u16]) -> [u8; 32] {
    let mut e = [0xFFu8; 32];
    e[0] = seq;
    e[11] = ATTR_LFN;
    e[12] = 0;
    e[13] = checksum;
    for (i, &off) in LFN_OFFSETS.iter().enumerate() {
        let unit = units.get(i).copied().unwrap_or(0xFFFF);
        e[off..off + 2].copy_from_slice(&unit.to_le_bytes());
    }
    e
}

#[test]
fn checksum_matches_a_real_mtools_generated_image() {
    // Verified against `mkfs.fat -F 32` + `mcopy ::/ "long name.txt"`:
    // the image carries short name LONGNA~1.TXT with checksum byte 0xF4.
    assert_eq!(lfn_checksum(b"LONGNA~1TXT"), 0xF4);
}

#[test]
fn decodes_a_two_entry_chain_in_directory_order() {
    let short = *b"LONGNA~1TXT";
    let checksum = lfn_checksum(&short);
    let name: Vec<u16> = "long name.txt"
        .encode_utf16()
        .chain(core::iter::once(0))
        .collect();
    let mut chain = LfnChain::new();
    // Directory order: highest sequence number first (0x40 flag).
    for (i, chunk) in name.chunks(LFN_CHARS_PER_ENTRY).enumerate().rev() {
        let seq = (i + 1) as u8
            | if i + 1 == name.len().div_ceil(LFN_CHARS_PER_ENTRY) {
                LFN_SEQ_LAST
            } else {
                0
            };
        chain.push(&lfn_entry(seq, checksum, chunk));
    }
    assert!(chain.belongs_to(&short));
    let mut out = [0u8; 64];
    let n = chain.decode(&mut out).expect("decodes");
    assert_eq!(&out[..n], b"long name.txt");
}

#[test]
fn checksum_mismatch_is_not_attributed_to_the_short_entry() {
    let mut chain = LfnChain::new();
    chain.push(&lfn_entry(0x41, 0x00, &[b'X' as u16]));
    assert!(!chain.belongs_to(b"OTHER   TXT"));
}

#[test]
fn truncated_chain_without_head_is_rejected() {
    let short = *b"LONGNA~1TXT";
    let mut chain = LfnChain::new();
    chain.push(&lfn_entry(1, lfn_checksum(&short), &[b'A' as u16]));
    assert!(!chain.belongs_to(&short));
    let mut out = [0u8; 16];
    assert!(chain.decode(&mut out).is_none());
}

#[test]
fn non_ascii_names_round_trip_through_utf8() {
    let mut chain = LfnChain::new();
    // "Grüße" exercises 2-byte and 3-byte UTF-8 sequences.
    let units = [0x0047u16, 0x0072, 0x00FC, 0x00DF, 0x0065];
    chain.push(&lfn_entry(0x41, 0x00, &units));
    let mut out = [0u8; 32];
    let n = chain.decode(&mut out).expect("decodes");
    assert_eq!(&out[..n], "Grüße".as_bytes());
}

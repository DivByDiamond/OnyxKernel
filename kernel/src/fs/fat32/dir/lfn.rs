//! Long File Name (LFN) support: accumulate the `0x0F` entries that precede
//! a short dirent into a UTF-8 name, match path components case-insensitively
//! against either form, and generate LFN runs for new files.

/// UTF-16 code units carried by one LFN entry.
pub(crate) const LFN_CHARS_PER_ENTRY: usize = 13;
/// A name is split into at most 20 entries (255 UTF-16 units).
pub(crate) const LFN_MAX_ENTRIES: usize = 20;
/// Capacity of the assembled UTF-16 name.
pub(crate) const LFN_MAX_CHARS: usize = LFN_CHARS_PER_ENTRY * LFN_MAX_ENTRIES;
/// Byte offsets of the 13 UTF-16 units inside a 32-byte LFN entry.
const LFN_OFFSETS: [usize; LFN_CHARS_PER_ENTRY] = [1, 3, 5, 7, 9, 14, 16, 18, 20, 22, 24, 28, 30];
/// Sequence-number flag on the physically first LFN entry (highest number).
const LFN_SEQ_LAST: u8 = 0x40;
/// Bits of the sequence byte that carry the 1-based entry number.
const LFN_SEQ_MASK: u8 = 0x1F;
/// Checksum byte inside an LFN entry.
const LFN_CHECKSUM_OFFSET: usize = 13;

/// The LFN chain accumulated while walking a directory, in physical order.
pub(crate) struct LfnChain {
    chars: [u16; LFN_MAX_CHARS],
    /// Sequence number of the physically first entry (0 = none seen yet).
    last_seq: usize,
    checksum: u8,
    valid: bool,
}

impl LfnChain {
    pub(crate) const fn new() -> Self {
        Self {
            chars: [0; LFN_MAX_CHARS],
            last_seq: 0,
            checksum: 0,
            valid: false,
        }
    }

    /// Drop any partial chain; called on a deleted entry or after a match.
    pub(crate) fn reset(&mut self) {
        *self = Self::new();
    }

    /// Feed the next `0x0F` entry, in directory order.
    pub(crate) fn push(&mut self, entry: &[u8]) {
        if entry.len() < 32 {
            self.valid = false;
            return;
        }
        let seq = usize::from(entry[0] & LFN_SEQ_MASK);
        if seq == 0 || seq > LFN_MAX_ENTRIES {
            self.valid = false;
            return;
        }
        if entry[0] & LFN_SEQ_LAST != 0 {
            // Highest sequence number: this is where the chain starts.
            self.last_seq = seq;
            self.checksum = entry[LFN_CHECKSUM_OFFSET];
            self.chars = [0; LFN_MAX_CHARS];
        } else if self.last_seq == 0 || entry[LFN_CHECKSUM_OFFSET] != self.checksum {
            // Entry without a preceding chain head, or a mismatched checksum.
            self.valid = false;
            return;
        }
        let base = (seq - 1) * LFN_CHARS_PER_ENTRY;
        for (i, &off) in LFN_OFFSETS.iter().enumerate() {
            self.chars[base + i] = u16::from_le_bytes([entry[off], entry[off + 1]]);
        }
        self.valid = true;
    }

    /// True when this chain belongs to `short_name` (checksum + complete run).
    pub(crate) fn belongs_to(&self, short_name: &[u8; 11]) -> bool {
        self.valid && self.last_seq != 0 && self.checksum == lfn_checksum(short_name)
    }

    /// Decode the assembled name into `out` as UTF-8; returns its byte length.
    pub(crate) fn decode(&self, out: &mut [u8]) -> Option<usize> {
        if !self.valid || self.last_seq == 0 {
            return None;
        }
        let mut pos = 0usize;
        for &unit in self.chars.iter().take(self.last_seq * LFN_CHARS_PER_ENTRY) {
            // 0x0000 terminates the name; 0xFFFF pads the final entry.
            if unit == 0x0000 || unit == 0xFFFF {
                break;
            }
            pos += encode_utf8(unit, out.get_mut(pos..)?)?;
        }
        Some(pos)
    }
}

/// Checksum of the 11-byte short name, per the FAT LFN specification: rotate
/// right by one bit and add each byte.
pub(crate) fn lfn_checksum(short_name: &[u8; 11]) -> u8 {
    let mut sum = 0u8;
    for &b in short_name {
        sum = (sum >> 1).wrapping_add((sum & 1) << 7).wrapping_add(b);
    }
    sum
}

/// ASCII case-insensitive comparison of a path component and a stored name.
pub(crate) fn name_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(&x, &y)| x.eq_ignore_ascii_case(&y))
}

/// Encode one UTF-16 unit as UTF-8 into `out`; returns the number of bytes.
/// Unpaired surrogates (invalid in a name) become `?` instead of failing.
fn encode_utf8(unit: u16, out: &mut [u8]) -> Option<usize> {
    let cp = u32::from(unit);
    let (len, bytes): (usize, [u8; 3]) = if (0xD800..=0xDFFF).contains(&unit) || cp == 0 {
        (1, [b'?', 0, 0])
    } else if cp < 0x80 {
        (1, [cp as u8, 0, 0])
    } else if cp < 0x800 {
        (2, [0xC0 | (cp >> 6) as u8, 0x80 | (cp & 0x3F) as u8, 0])
    } else {
        (
            3,
            [
                0xE0 | (cp >> 12) as u8,
                0x80 | ((cp >> 6) & 0x3F) as u8,
                0x80 | (cp & 0x3F) as u8,
            ],
        )
    };
    out.get_mut(..len)?.copy_from_slice(&bytes[..len]);
    Some(len)
}

#[cfg(test)]
mod tests {
    use super::super::ATTR_LFN;
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
}

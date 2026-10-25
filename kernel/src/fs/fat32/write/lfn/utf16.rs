//! UTF-8 → UTF-16 conversion for LFN entry payloads.
use super::super::super::dir::lfn::LFN_MAX_CHARS;

/// Encode `name` as UTF-16 code units for the LFN entries.
pub(super) fn utf16_units(name: &[u8], out: &mut [u16; LFN_MAX_CHARS]) -> usize {
    let mut n = 0usize;
    let mut i = 0usize;
    while i < name.len() && n < out.len() {
        let (cp, used) = decode_utf8(&name[i..]);
        i += used;
        if cp > 0xFFFF {
            if n + 2 > out.len() {
                break;
            }
            let v = cp - 0x1_0000;
            out[n] = 0xD800 | (v >> 10) as u16;
            out[n + 1] = 0xDC00 | (v & 0x3FF) as u16;
            n += 2;
        } else {
            out[n] = cp as u16;
            n += 1;
        }
    }
    n
}

/// Decode one UTF-8 scalar; invalid sequences fall back to Latin-1 per byte so
/// a malformed name is still represented instead of being dropped.
fn decode_utf8(bytes: &[u8]) -> (u32, usize) {
    let b0 = bytes[0];
    let (need, mut cp) = match b0 {
        0x00..=0x7F => return (u32::from(b0), 1),
        0xC2..=0xDF => (1, u32::from(b0 & 0x1F)),
        0xE0..=0xEF => (2, u32::from(b0 & 0x0F)),
        0xF0..=0xF4 => (3, u32::from(b0 & 0x07)),
        _ => return (u32::from(b0), 1),
    };
    if bytes.len() <= need {
        return (u32::from(b0), 1);
    }
    for &cont in &bytes[1..=need] {
        if cont & 0xC0 != 0x80 {
            return (u32::from(b0), 1);
        }
        cp = (cp << 6) | u32::from(cont & 0x3F);
    }
    (cp, need + 1)
}

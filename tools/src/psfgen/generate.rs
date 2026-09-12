use crate::fontdata::*;

const PSF1_MAGIC: u16 = 0x0436;
const GLYPHS: usize = 256;
const GLYPH_H: usize = 16;

/// Blank cell: rendered for space, not the "unknown glyph" placeholder box.
/// A solid box in place of every space made the console unreadable — it
/// looked like screen corruption rather than whitespace (found while
/// verifying the blank-console timer/font fix, 2026-09-12).
const BLANK: [u8; 16] = [0; 16];

fn glyph_bitmap(c: u8) -> &'static [u8; 16] {
    match c {
        b'A'..=b'Z' => &ALPHA_UPPER[(c - b'A') as usize],
        b'a'..=b'z' => &ALPHA_LOWER[(c - b'a') as usize],
        b'0'..=b'9' => &DIGITS[(c - b'0') as usize],
        b' ' => &BLANK,
        // Full 7-bit ASCII coverage (2026-09-12): 0x00-0x1F (control codes)
        // and 0x7F (DEL) are non-printable by definition — the ANSI parser
        // (fb_term/ansi/parse.rs) already intercepts the ones with real
        // meaning (\n \r \t ESC) before they ever reach the glyph draw
        // path, so this only fires for stray/unhandled control bytes. A
        // real terminal renders those as nothing, not a visible mark, so
        // they get the same blank cell as space rather than the "unknown
        // glyph" placeholder box (which stays reserved for genuinely
        // unmapped bytes — extended/non-ASCII, ≥ 0x80).
        0x00..=0x1F | 0x7F => &BLANK,
        _ => punct_glyph(c).unwrap_or(&GLYPH_DEFAULT),
    }
}

fn unicode_table() -> Vec<u8> {
    let mut table = Vec::new();
    for cp in 0..GLYPHS {
        table.extend_from_slice(&(cp as u16).to_le_bytes());
        table.extend_from_slice(&0xFFFFu16.to_le_bytes());
    }
    table
}

pub fn psf1() -> Vec<u8> {
    let charsize = GLYPH_H as u32;
    let ut = unicode_table();
    let mode: u8 = 0x02;
    let total_size = 4 + GLYPHS * GLYPH_H + ut.len();
    let mut buf = Vec::with_capacity(total_size);
    buf.extend_from_slice(&PSF1_MAGIC.to_le_bytes());
    buf.push(mode);
    buf.push(charsize as u8);
    for c in 0..GLYPHS {
        buf.extend_from_slice(glyph_bitmap(c as u8));
    }
    buf.extend_from_slice(&ut);
    assert_eq!(
        buf.len(),
        total_size,
        "psf1: generated buffer size drifted from the declared header size"
    );
    buf
}

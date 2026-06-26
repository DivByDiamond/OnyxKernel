use crate::fontdata::*;

const PSF1_MAGIC: u16 = 0x0436;
const GLYPHS: usize = 256;
const GLYPH_H: usize = 16;

fn glyph_bitmap(c: u8) -> &'static [u8; 16] {
    match c {
        b'A'..=b'Z' => &ALPHA_UPPER[(c - b'A') as usize],
        b'a'..=b'z' => &ALPHA_LOWER[(c - b'a') as usize],
        b'0'..=b'9' => &DIGITS[(c - b'0') as usize],
        _ => &GLYPH_DEFAULT,
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
    for c in 0..GLYPHS as u8 { buf.extend_from_slice(glyph_bitmap(c)); }
    buf.extend_from_slice(&ut);
    buf
}

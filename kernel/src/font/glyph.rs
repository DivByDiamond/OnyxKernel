use super::shared::FONT_GLYPH_BYTES;
use super::shared::{G_FONT, G_UNI_MAP, G_UNI_MAP_LEN};

#[derive(Clone, Copy)]
pub struct GlyphData {
    pub data: *const u8,
    pub charsize: u32,
    pub width: u32,
    pub height: u32,
}

pub fn glyph_for_unicode(cp: u32) -> Option<u32> {
    if cp < 256 {
        // SAFETY: G_FONT is plain data set once by font::init during boot, read-only afterwards.
        let f = unsafe { G_FONT? };
        if cp < f.num_glyphs {
            return Some(cp);
        }
    }
    // SAFETY: G_UNI_MAP/G_UNI_MAP_LEN are set once by the boot font parser; iterating .take(G_UNI_MAP_LEN) stays inside the fixed array.
    unsafe {
        for entry in G_UNI_MAP.iter().take(G_UNI_MAP_LEN) {
            if entry.codepoint == cp {
                return Some(entry.glyph_idx);
            }
        }
    }
    None
}

pub fn glyph_bitmap(c: u8) -> &'static [u8; FONT_GLYPH_BYTES] {
    // SAFETY: G_FONT is set once during boot; idx is clamped to num_glyphs-1 and both parsers validate that the glyph area fits the blob, so `ptr` stays inside the loaded PSF data (readers consume height <= FONT_GLYPH_BYTES rows).
    unsafe {
        if let Some(f) = G_FONT {
            let idx = (c as u32).min(f.num_glyphs - 1) as usize;
            let off = idx * f.charsize as usize;
            let ptr = f.glyphs.add(off) as *const [u8; FONT_GLYPH_BYTES];
            &*ptr
        } else {
            &BLANK_GLYPH
        }
    }
}

pub fn glyph_bitmap_unicode(cp: u32) -> GlyphData {
    if let Some(idx) = glyph_for_unicode(cp) {
        // SAFETY: G_FONT set once during boot; safe_idx clamped to num_glyphs-1 and the glyph area was validated by the parsers, so data stays inside the blob (consumers bounds-check reads against charsize).
        unsafe {
            if let Some(f) = G_FONT {
                let safe_idx = (idx as usize).min(f.num_glyphs as usize - 1);
                let off = safe_idx * f.charsize as usize;
                return GlyphData {
                    data: f.glyphs.add(off),
                    charsize: f.charsize,
                    width: f.width,
                    height: f.height,
                };
            }
        }
    }
    // SAFETY: G_FONT set once during boot; fallback offset clamps b'?' to num_glyphs-1 inside the validated glyph area, and the BLANK_GLYPH arm touches only a static array.
    unsafe {
        if let Some(f) = G_FONT {
            let off = (b'?' as usize).min(f.num_glyphs as usize - 1) * f.charsize as usize;
            GlyphData {
                data: f.glyphs.add(off),
                charsize: f.charsize,
                width: f.width,
                height: f.height,
            }
        } else {
            GlyphData {
                data: BLANK_GLYPH.as_ptr(),
                charsize: FONT_GLYPH_BYTES as u32,
                width: super::shared::FONT_W as u32,
                height: super::shared::FONT_H as u32,
            }
        }
    }
}

pub fn glyph_for_cp(cp: u32) -> Option<u8> {
    // SAFETY: G_FONT is set once during boot; the u16 table walk checks pos + 1 < unicode_len before each f.unicode.add read and caps glyph at num_glyphs.
    unsafe {
        let f = G_FONT?;
        if f.unicode.is_null() || f.unicode_len == 0 {
            return (cp as u8 <= 0x7F || (f.num_glyphs > 256 && cp < 256)).then_some(cp as u8);
        }
        let mut pos = 0usize;
        let mut glyph: u32 = 0;
        while pos + 1 < f.unicode_len && glyph < f.num_glyphs {
            let val = u16::from_le_bytes([*f.unicode.add(pos), *f.unicode.add(pos + 1)]);
            pos += 2;
            if val == 0xFFFF {
                glyph += 1;
            } else if val == 0xFFFE {
            } else if val as u32 == cp {
                return Some(glyph as u8);
            }
        }
        None
    }
}

pub fn glyph_or_default(cp: u32) -> u8 {
    glyph_for_cp(cp).unwrap_or(if cp < 256 { cp as u8 } else { b'?' })
}

static BLANK_GLYPH: [u8; FONT_GLYPH_BYTES] = [0u8; FONT_GLYPH_BYTES];

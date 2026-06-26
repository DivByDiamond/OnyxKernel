pub const FONT_W: usize = 8;
pub const FONT_H: usize = 16;
pub const FONT_NUM_GLYPHS: usize = 256;
pub const FONT_GLYPH_BYTES: usize = FONT_H;

#[derive(Clone, Copy)]
pub struct PcfFont {
    pub width: u32,
    pub height: u32,
    pub charsize: u32,
    pub num_glyphs: u32,
    pub glyphs: *const u8,
    pub unicode: *const u8,
    pub unicode_len: usize,
}

const UNICODE_MAP_SIZE: usize = 512;

#[derive(Clone, Copy)]
pub(crate) struct UniMapEntry {
    pub(crate) codepoint: u32,
    pub(crate) glyph_idx: u32,
}

pub(crate) static mut G_FONT: Option<PcfFont> = None;
pub(crate) static mut G_UNI_MAP: [UniMapEntry; UNICODE_MAP_SIZE] =
    [UniMapEntry { codepoint: 0, glyph_idx: 0 }; UNICODE_MAP_SIZE];
pub(crate) static mut G_UNI_MAP_LEN: usize = 0;

pub(crate) unsafe fn uni_map_insert(cp: u32, idx: u32) {
    if G_UNI_MAP_LEN < UNICODE_MAP_SIZE {
        G_UNI_MAP[G_UNI_MAP_LEN] = UniMapEntry {
            codepoint: cp,
            glyph_idx: idx,
        };
        G_UNI_MAP_LEN += 1;
    }
}

pub fn font() -> Option<PcfFont> {
    unsafe { G_FONT }
}

pub fn font_height() -> usize {
    unsafe { G_FONT.map(|f| f.height as usize).unwrap_or(FONT_H) }
}

pub fn font_width() -> usize {
    unsafe { G_FONT.map(|f| f.width as usize).unwrap_or(FONT_W) }
}

pub fn font_charsize() -> usize {
    unsafe { G_FONT.map(|f| f.charsize as usize).unwrap_or(FONT_GLYPH_BYTES) }
}

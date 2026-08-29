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
pub(crate) static mut G_UNI_MAP: [UniMapEntry; UNICODE_MAP_SIZE] = [UniMapEntry {
    codepoint: 0,
    glyph_idx: 0,
}; UNICODE_MAP_SIZE];
pub(crate) static mut G_UNI_MAP_LEN: usize = 0;

/// # Safety
///
/// Caller must be the single-threaded boot font parser (font::init before
/// secondary harts are released); G_UNI_MAP/G_UNI_MAP_LEN are only written
/// on that path and merely read afterwards, and the len check keeps the
/// write inside the fixed UNICODE_MAP_SIZE array.
pub(crate) unsafe fn uni_map_insert(cp: u32, idx: u32) {
    unsafe {
        if G_UNI_MAP_LEN < UNICODE_MAP_SIZE {
            G_UNI_MAP[G_UNI_MAP_LEN] = UniMapEntry {
                codepoint: cp,
                glyph_idx: idx,
            };
            G_UNI_MAP_LEN += 1;
        }
    }
}

pub fn font() -> Option<PcfFont> {
    // SAFETY: G_FONT is plain-data (Copy Option<PcfFont>) written once by font::init during single-threaded boot, read-only afterwards.
    unsafe { G_FONT }
}

pub fn font_height() -> usize {
    // SAFETY: G_FONT is set once during boot before secondary harts start; reading plain-data static mut has no concurrent writers.
    unsafe { G_FONT.map(|f| f.height as usize).unwrap_or(FONT_H) }
}

pub fn font_width() -> usize {
    // SAFETY: G_FONT is set once during boot before secondary harts start; reading plain-data static mut has no concurrent writes.
    unsafe { G_FONT.map(|f| f.width as usize).unwrap_or(FONT_W) }
}

pub fn font_charsize() -> usize {
    // SAFETY: G_FONT is set once during boot before secondary harts start; reading plain-data static mut has no concurrent writes.
    unsafe {
        G_FONT
            .map(|f| f.charsize as usize)
            .unwrap_or(FONT_GLYPH_BYTES)
    }
}

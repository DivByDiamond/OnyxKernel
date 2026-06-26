mod shared;
mod psf1;
mod psf2;
mod glyph;

pub use shared::{
    FONT_W, FONT_H, FONT_NUM_GLYPHS, FONT_GLYPH_BYTES,
    PcfFont, font, font_height, font_width, font_charsize,
};
pub use glyph::{
    GlyphData, glyph_bitmap, glyph_bitmap_unicode,
    glyph_for_unicode, glyph_for_cp, glyph_or_default,
};

use onyx_core::errno::{Errno, KResult};

pub unsafe fn init(data: &[u8]) -> KResult<()> {
    if data.len() < 4 { return Err(Errno::Io); }
    let magic = u32::from_le_bytes(data[..4].try_into().unwrap());
    if magic == 0x0436 || (magic & 0xFFFF) == 0x0436 {
        psf1::init_psf1(data)
    } else if magic == 0x864ab572 {
        psf2::init_psf2(data)
    } else {
        Err(Errno::NoEnt)
    }
}

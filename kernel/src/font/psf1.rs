use super::shared::{uni_map_insert, PcfFont, G_FONT};
use onyx_core::errno::{Errno, KResult};

pub(super) unsafe fn init_psf1(data: &[u8]) -> KResult<()> {
    if data.len() < 4 {
        return Err(Errno::Io);
    }
    let magic = u16::from_le_bytes(data[..2].try_into().unwrap());
    if magic != 0x0436 {
        return Err(Errno::NoEnt);
    }
    let mode = data[2];
    let charsize = data[3] as u32;
    let num_glyphs: u32 = if mode & 0x01 != 0 { 512 } else { 256 };
    let glyph_bytes = (num_glyphs as usize) * (charsize as usize);
    if data.len() < 4 + glyph_bytes {
        return Err(Errno::Io);
    }
    let (unicode_ptr, unicode_len) = if mode & 0x02 != 0 && data.len() > 4 + glyph_bytes {
        let ustart = 4 + glyph_bytes;
        (data.as_ptr().add(ustart), data.len() - ustart)
    } else {
        (core::ptr::null(), 0)
    };
    G_FONT = Some(PcfFont {
        width: 8,
        height: charsize,
        charsize,
        num_glyphs,
        glyphs: data.as_ptr().add(4),
        unicode: unicode_ptr,
        unicode_len,
    });
    if mode & 0x02 != 0 {
        parse_psf1_unicode_table(data, 4, num_glyphs, charsize);
    }
    Ok(())
}

unsafe fn parse_psf1_unicode_table(data: &[u8], hdr_size: usize, num_glyphs: u32, charsize: u32) {
    let glyph_bytes = (num_glyphs as usize) * (charsize as usize);
    let table_start = hdr_size + glyph_bytes;
    if table_start + 2 > data.len() {
        return;
    }
    let table = &data[table_start..];
    let mut glyph_idx = 0u32;
    let mut i = 0usize;
    while i + 1 < table.len() && glyph_idx < num_glyphs {
        let lo = table[i] as u16;
        let hi = table[i + 1] as u16;
        let val = (hi << 8) | lo;
        i += 2;
        if val == 0xFFFF {
            glyph_idx += 1;
            continue;
        }
        if val == 0xFFFE {
            continue;
        }
        let cp = val as u32;
        if cp >= 256 {
            uni_map_insert(cp, glyph_idx);
        }
    }
}

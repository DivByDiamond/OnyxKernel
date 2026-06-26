use super::shared::{PcfFont, G_FONT, uni_map_insert};
use onyx_core::errno::{Errno, KResult};

const PSF2_HAS_UNICODE_TABLE: u32 = 1;

pub(super) unsafe fn init_psf2(data: &[u8]) -> KResult<()> {
    if data.len() < 32 {
        return Err(Errno::Io);
    }
    let _version = u32::from_le_bytes(data[4..8].try_into().unwrap());
    let hdr_size = u32::from_le_bytes(data[8..12].try_into().unwrap()) as usize;
    let flags = u32::from_le_bytes(data[12..16].try_into().unwrap());
    let num_glyphs = u32::from_le_bytes(data[16..20].try_into().unwrap());
    let charsize = u32::from_le_bytes(data[20..24].try_into().unwrap());
    let height = u32::from_le_bytes(data[24..28].try_into().unwrap());
    let width = u32::from_le_bytes(data[28..32].try_into().unwrap());
    let glyph_bytes = (num_glyphs as usize) * (charsize as usize);
    let end = hdr_size + glyph_bytes;
    if data.len() < end {
        return Err(Errno::Io);
    }
    let (unicode_ptr, unicode_len) = if data.len() > end {
        (data.as_ptr().add(end), data.len() - end)
    } else {
        (core::ptr::null(), 0)
    };
    G_FONT = Some(PcfFont {
        width,
        height,
        charsize,
        num_glyphs,
        glyphs: data.as_ptr().add(hdr_size),
        unicode: unicode_ptr,
        unicode_len,
    });
    if flags & PSF2_HAS_UNICODE_TABLE != 0 {
        parse_psf2_unicode_table(data, hdr_size, num_glyphs, charsize);
    }
    Ok(())
}

unsafe fn parse_psf2_unicode_table(data: &[u8], hdr_size: usize, num_glyphs: u32, charsize: u32) {
    let glyph_bytes = (num_glyphs as usize) * (charsize as usize);
    let table_start = hdr_size + glyph_bytes;
    if table_start >= data.len() {
        return;
    }
    let table = &data[table_start..];
    let mut glyph_idx = 0u32;
    let mut i = 0usize;
    while i < table.len() && glyph_idx < num_glyphs {
        let b = table[i];
        if b == 0xFF {
            glyph_idx += 1;
            i += 1;
            continue;
        }
        if b == 0xFE {
            i += 1;
            continue;
        }
        let cp = decode_utf8(table, &mut i);
        if cp != 0 && cp >= 256 {
            uni_map_insert(cp, glyph_idx);
        }
        while i < table.len() && table[i] != 0xFF && table[i] != 0xFE && table[i] != 0 {
            i += 1;
        }
        if i < table.len() && table[i] == 0 {
            i += 1;
        }
    }
}

unsafe fn decode_utf8(data: &[u8], pos: &mut usize) -> u32 {
    if *pos >= data.len() {
        return 0;
    }
    let b0 = data[*pos];
    if b0 < 0x80 {
        *pos += 1;
        return b0 as u32;
    }
    let (mask, n) = if b0 < 0xE0 {
        (0x1Fu8, 2)
    } else if b0 < 0xF0 {
        (0x0F, 3)
    } else {
        (0x07, 4)
    };
    let mut cp = (b0 & mask) as u32;
    for _ in 1..n {
        *pos += 1;
        if *pos >= data.len() {
            return 0;
        }
        cp = (cp << 6) | ((data[*pos] & 0x3F) as u32);
    }
    *pos += 1;
    cp
}

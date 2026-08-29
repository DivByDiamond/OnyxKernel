use super::shared::{G_FONT, PcfFont, uni_map_insert};
use onyx_core::errno::{Errno, KResult};

const PSF2_MAGIC: u32 = 0x864ab572;
const PSF2_HAS_UNICODE_TABLE: u32 = 1;
/// Sanity caps for untrusted header fields: a crafted /font/default.psf must
/// never hang or corrupt the kernel, just fail the load.
const MAX_GLYPHS: u32 = 4096;
const MAX_CHARSIZE: u32 = 256;
const MAX_DIM: u32 = 256;

fn read_u32(data: &[u8], off: usize) -> KResult<u32> {
    data.get(off..off + 4)
        .and_then(|b| <[u8; 4]>::try_from(b).ok())
        .map(u32::from_le_bytes)
        .ok_or(Errno::Io)
}

/// # Safety
///
/// Caller (font::init) must run once, single-threaded, during boot before
/// secondary harts start; `data` must remain valid for the kernel's
/// lifetime since G_FONT stores pointers into it. hdr_size/num_glyphs/
/// charsize are sanity-capped and the glyph area is length- and
/// overflow-checked before any as_ptr().add().
pub(super) unsafe fn init_psf2(data: &[u8]) -> KResult<()> {
    unsafe {
        if data.len() < 32 {
            return Err(Errno::Io);
        }
        let magic = read_u32(data, 0)?;
        if magic != PSF2_MAGIC {
            return Err(Errno::NoEnt);
        }
        // _version = read_u32(data, 4)?; — unused
        let hdr_size = read_u32(data, 8)? as usize;
        let flags = read_u32(data, 12)?;
        let num_glyphs = read_u32(data, 16)?;
        let charsize = read_u32(data, 20)?;
        let height = read_u32(data, 24)?;
        let width = read_u32(data, 28)?;

        if hdr_size < 32 || hdr_size > data.len() {
            return Err(Errno::Inval);
        }
        if num_glyphs == 0 || num_glyphs > MAX_GLYPHS {
            return Err(Errno::Inval);
        }
        if charsize == 0
            || charsize > MAX_CHARSIZE
            || width == 0
            || width > MAX_DIM
            || height == 0
            || height > MAX_DIM
        {
            return Err(Errno::Inval);
        }
        let glyph_bytes = (num_glyphs as usize)
            .checked_mul(charsize as usize)
            .ok_or(Errno::Inval)?;
        let end = hdr_size.checked_add(glyph_bytes).ok_or(Errno::Inval)?;
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
}

/// # Safety
///
/// Caller must have validated the glyph area of `data`; the table start is
/// re-checked here (`table_start >= data.len()` bails out), `table` is a
/// real subslice, and every access goes through bounds-checked indexing.
unsafe fn parse_psf2_unicode_table(data: &[u8], hdr_size: usize, num_glyphs: u32, charsize: u32) {
    unsafe {
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
}

/// # Safety
///
/// `*pos` must be a valid cursor into `data` (<= data.len()); the function
/// re-checks `*pos < data.len()` before every byte read and only advances
/// `pos` after those checks, so no out-of-bounds access is possible.
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

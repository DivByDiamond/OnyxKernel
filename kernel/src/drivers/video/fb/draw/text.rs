use super::super::{put_pixel, put_pixel_front, width};
use crate::font;

pub fn draw_char(x: usize, y: usize, c: u8, fg: u32, bg: u32) {
    let glyph = font::glyph_bitmap(c);
    for (row, &bits) in glyph.iter().enumerate() {
        for col in 0..font::FONT_W {
            let on = (bits >> (7 - col)) & 1;
            put_pixel(x + col, y + row, if on != 0 { fg } else { bg });
        }
    }
}

/// Present-time software cursor: paint a solid block (the classic inverted
/// cell) over one character cell ON THE VISIBLE FRONT BUFFER. Called by the
/// console presenter right after [`present`]; because every present starts
/// by copying the back buffer over the front, the block never bakes into
/// the frame content and needs no save/restore bookkeeping.
///
/// `block` is the terminal's effective foreground color (the old in-kernel
/// cursor drew a space glyph with swapped colors, which resolves to exactly
/// this solid fill).
pub fn draw_cursor_cell_front(col: usize, row: usize, block: u32) {
    let fw = font::FONT_W;
    let fh = font::FONT_H;
    let x0 = col * fw;
    let y0 = row * fh;
    for dy in 0..fh {
        for dx in 0..fw {
            let (x, y) = (x0 + dx, y0 + dy);
            if x < width() && y < super::super::height() {
                put_pixel_front(x, y, block);
            }
        }
    }
}

pub fn draw_unicode_char(x: usize, y: usize, cp: u32, fg: u32, bg: u32) {
    let gd = font::glyph_bitmap_unicode(cp);
    let fh = gd.height as usize;
    let fw = gd.width as usize;
    let bytes_per_row = fw.div_ceil(8);
    for row in 0..fh {
        let row_off = row * bytes_per_row;
        for col in 0..fw {
            let byte_idx = col / 8;
            let bit_idx = 7 - (col % 8);
            let bits = unsafe {
                // SAFETY: gd.data points at a glyph inside the loaded font bitmap (font::glyph_bitmap_unicode) and off is bounds-checked against gd.charsize before the read.
                let off = row_off + byte_idx;
                if off < gd.charsize as usize {
                    *gd.data.add(off)
                } else {
                    0
                }
            };
            let on = (bits >> bit_idx) & 1;
            put_pixel(x + col, y + row, if on != 0 { fg } else { bg });
        }
    }
}

pub fn draw_str(mut x: usize, y: usize, s: &str, fg: u32, bg: u32) {
    for &b in s.as_bytes() {
        match b {
            b'\n' => return,
            b'\r' => x = 0,
            b'\t' => x = (x / (4 * font::FONT_W) + 1) * (4 * font::FONT_W),
            _ => {
                if x + font::FONT_W > width() {
                    return;
                }
                draw_char(x, y, b, fg, bg);
                x += font::FONT_W;
            }
        }
    }
}

pub fn draw_unicode_str(mut x: usize, y: usize, s: &str, fg: u32, bg: u32) {
    let fw = font::font_width();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        match b {
            b'\n' => return,
            b'\r' => {
                x = 0;
                i += 1;
                continue;
            }
            b'\t' => {
                x = (x / (4 * fw) + 1) * (4 * fw);
                i += 1;
                continue;
            }
            _ => {}
        }
        let cp;
        if b < 0x80 {
            cp = b as u32;
            i += 1;
        } else if b < 0xE0 {
            if i + 1 >= bytes.len() {
                break;
            }
            cp = ((b & 0x1F) as u32) << 6 | ((bytes[i + 1] & 0x3F) as u32);
            i += 2;
        } else if b < 0xF0 {
            if i + 2 >= bytes.len() {
                break;
            }
            cp = ((b & 0x0F) as u32) << 12
                | ((bytes[i + 1] & 0x3F) as u32) << 6
                | ((bytes[i + 2] & 0x3F) as u32);
            i += 3;
        } else {
            if i + 3 >= bytes.len() {
                break;
            }
            cp = ((b & 0x07) as u32) << 18
                | ((bytes[i + 1] & 0x3F) as u32) << 12
                | ((bytes[i + 2] & 0x3F) as u32) << 6
                | ((bytes[i + 3] & 0x3F) as u32);
            i += 4;
        }
        if x + fw > width() {
            return;
        }
        draw_unicode_char(x, y, cp, fg, bg);
        x += fw;
    }
}

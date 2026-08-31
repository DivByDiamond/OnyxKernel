use super::{put_pixel, size_bytes, width};
use crate::font;

// ── Double buffering (todo P3 #2) ────────────────────────────────────────
//
// The back buffer is physically contiguous RAM (pmm::alloc_n) addressed
// through the kernel's direct physical mapping, exactly like the front
// buffer pointer. It is allocated lazily on the first get_back_buffer()
// call: a full 1280x720x4 surface is ~3.7 MB (896 pages), which would
// starve the 4 MB kernel heap if it were kmalloc'd. If the contiguous
// allocation fails (fragmented or tiny RAM), get_back_buffer returns the
// front buffer so callers keep working in single-buffer mode, and
// swap_buffers degrades to a no-op.

use core::sync::atomic::{AtomicUsize, Ordering};

static G_BACK_PA: AtomicUsize = AtomicUsize::new(0);

/// Back-buffer pointer (kernel direct-mapped VA == PA), or the front
/// buffer when no back buffer could be allocated.
pub fn get_back_buffer() -> *mut u32 {
    let mut pa = G_BACK_PA.load(Ordering::Acquire) as u64;
    if pa == 0 {
        let pages = size_bytes().div_ceil(crate::mm::pmm::PAGE_SIZE);
        // Lazy one-time allocation from arbitrary kernel context. The PMM
        // lock serialises concurrent callers; on failure we retry on the
        // next call rather than caching an error.
        // SAFETY: pmm::init has completed by the time a framebuffer exists;
        // alloc_n self-locks and zeroes the returned run.
        let alloc = unsafe { crate::mm::pmm::alloc_n(pages) };
        if let Ok(back) = alloc {
            // Only publish on success so a lost race never overwrites a
            // previously published buffer.
            if G_BACK_PA
                .compare_exchange(0, back as usize, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                pa = back;
            } else {
                // Another hart won the race: return our (now orphaned)
                // allocation to the pool before using theirs.
                // SAFETY: `back` is a page-aligned allocation we own and
                // never published; free_unlocked revalidates the range.
                unsafe { crate::mm::pmm::free(back) };
                pa = G_BACK_PA.load(Ordering::Acquire) as u64;
            }
        }
    }
    if pa == 0 {
        return super::fb_base_ptr() as *mut u32;
    }
    pa as *mut u32
}

/// Present the back buffer: copy it over the visible front buffer.
/// Call after a full redraw to eliminate tearing. No-op when running in
/// single-buffer fallback mode (see get_back_buffer).
pub fn swap_buffers() {
    let pa = G_BACK_PA.load(Ordering::Acquire) as u64;
    if pa == 0 {
        return;
    }
    let bytes = size_bytes();
    // SAFETY: both pointers are kernel direct-mapped RAM of size_bytes():
    // the front buffer is the installed framebuffer surface and the back
    // buffer is a pmm-owned contiguous run; non-overlapping by construction.
    unsafe {
        core::ptr::copy_nonoverlapping(pa as *const u8, super::fb_base_ptr(), bytes);
    }
}

pub fn draw_char(x: usize, y: usize, c: u8, fg: u32, bg: u32) {
    let glyph = font::glyph_bitmap(c);
    for (row, &bits) in glyph.iter().enumerate() {
        for col in 0..font::FONT_W {
            let on = (bits >> (7 - col)) & 1;
            put_pixel(x + col, y + row, if on != 0 { fg } else { bg });
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

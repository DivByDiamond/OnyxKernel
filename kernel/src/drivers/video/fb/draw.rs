use super::{front_base, put_pixel, put_pixel_front, size_bytes, width};
use crate::font;

// ── Double buffering (TUI anti-flicker) ──────────────────────────────────
//
// The ANSI console (fb_term) paints every byte it receives the moment it
// arrives, so a full-screen TUI redraw (`ESC[2J` + repaint) used to show
// cleared and partially-drawn states on the visible framebuffer for tens
// of milliseconds — perceived as heavy flicker in vim/otop/oed/osnake
// (mid-redraw frames: status line missing, cursor block at an intermediate
// position). The fix: console output paints into an off-screen BACK buffer
// of identical geometry, and a debounced presenter
// (fb_term::ansi::present_tick, driven from the 100 Hz timer tick) copies
// the finished frame to the visible front buffer only after console output
// has settled.
//
// The back buffer is physically contiguous RAM (pmm::alloc_n) addressed
// through the kernel's direct physical mapping, exactly like the front
// buffer pointer. It is allocated EAGERLY by fb::init/init_device
// (single-threaded boot / set_mode context) — the presenter runs from
// timer-interrupt context and must never allocate. If the contiguous
// allocation fails, double_buffered() stays false and the console
// transparently falls back to direct-to-front painting (legacy behavior:
// fully functional, but the redraw flicker remains).

use core::sync::atomic::{AtomicUsize, Ordering};

static G_BACK_PA: AtomicUsize = AtomicUsize::new(0);
/// Pages owned by the published back-buffer run (for free on re-init).
static G_BACK_PAGES: AtomicUsize = AtomicUsize::new(0);

/// Eagerly allocate the console back buffer for the CURRENT fb geometry.
/// Called from fb::init / fb::init_device right after the front geometry
/// is installed; safe to call again on re-init (set_mode resize path):
/// the stale run is freed first. On allocation failure the console runs
/// in direct (single-buffer) mode.
pub fn init_back_buffer() {
    let pages = size_bytes().div_ceil(crate::mm::pmm::PAGE_SIZE);
    let old_pa = G_BACK_PA.swap(0, Ordering::AcqRel) as u64;
    if old_pa != 0 {
        let old_pages = G_BACK_PAGES.swap(0, Ordering::AcqRel);
        if old_pages != 0 {
            // SAFETY: old_pa is a page-aligned pmm run this module published
            // earlier (init_back_buffer) and never republished; free
            // revalidates the range.
            unsafe { crate::mm::pmm::free(old_pa) };
        }
    }
    // SAFETY: pmm::init has completed by the time a framebuffer exists
    // (fb init runs from display/display-driver init); alloc_n self-locks
    // and zeroes the returned run, so the back buffer starts black.
    match unsafe { crate::mm::pmm::alloc_n(pages) } {
        Ok(back) => {
            G_BACK_PAGES.store(pages, Ordering::Release);
            G_BACK_PA.store(back as usize, Ordering::Release);
        }
        Err(_) => {
            crate::kwrn!(
                "fb",
                "no contiguous back buffer; console double buffering disabled"
            );
        }
    }
}

/// True when a separate back buffer exists (double-buffered console).
/// The console presenter checks this on every tick; false means legacy
/// direct-to-front painting.
#[inline]
pub fn double_buffered() -> bool {
    G_BACK_PA.load(Ordering::Acquire) != 0
}

/// Test-only hook: publish a host-provided back-buffer address so host
/// integration tests can exercise paint→present without pmm.
#[cfg(test)]
pub fn test_publish_back(pa: usize) {
    G_BACK_PA.store(pa, Ordering::Release);
    G_BACK_PAGES.store(1, Ordering::Release);
}

/// Test-only hook: drop the published back buffer (host tests).
#[cfg(test)]
pub fn test_clear_back() {
    G_BACK_PA.store(0, Ordering::Release);
    G_BACK_PAGES.store(0, Ordering::Release);
}

/// Paint-target base for console rendering: the off-screen back buffer
/// when double buffering is active, else the visible front buffer.
#[inline]
pub fn paint_base() -> *mut u8 {
    let pa = G_BACK_PA.load(Ordering::Acquire);
    if pa != 0 { pa as *mut u8 } else { front_base() }
}

/// Present the last painted frame: copy the back buffer over the visible
/// front buffer. No-op in direct (single-buffer) mode. Called by the
/// console presenter with the console write lock held, so a frame is
/// never copied mid-write.
pub fn present() {
    let pa = G_BACK_PA.load(Ordering::Acquire);
    if pa == 0 {
        return;
    }
    let bytes = size_bytes();
    // SAFETY: both pointers are kernel direct-mapped memory of size_bytes():
    // the front buffer is the installed framebuffer surface and the back
    // buffer is a pmm-owned contiguous run; non-overlapping by construction.
    unsafe {
        core::ptr::copy_nonoverlapping(pa as *const u8, front_base(), bytes);
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
            if x < width() && y < super::height() {
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

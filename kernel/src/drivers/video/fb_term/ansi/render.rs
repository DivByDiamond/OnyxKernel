//! Pixel-level rendering for the ANSI console: character drawing, erase
//! operations, scroll-region blits and the software cursor block.
//!
//! State (cursor position, colors, scroll region) lives in
//! [`AnsiTerm`](super::AnsiTerm); this module only paints the framebuffer.
//! The alt-screen surface swap (CSI ?1049) also lives here because it is a
//! whole-surface copy/fill operation.

use super::state::{AnsiTerm, FONT_H, FONT_W};
use crate::drivers::fb;
use core::sync::atomic::{AtomicUsize, Ordering};

/// VA of the saved normal-screen surface (0 = not allocated / no fb).
static G_SAVE_PA: AtomicUsize = AtomicUsize::new(0);

impl AnsiTerm {
    /// Scroll the region up by one line (content moves up).
    pub(super) fn scroll_up(&mut self) {
        let (_, pitch, bpp, _, base) = fb_info();
        if bpp != 32 || base == 0 {
            return;
        }
        let row_bytes = pitch * FONT_H;
        // Move rows top+1..=bot up by one.
        for row in self.top..self.bot {
            let dst = base + row * row_bytes;
            let src = base + (row + 1) * row_bytes;
            // SAFETY: base/pitch come from fb::info() (validated fb geometry), rows stay inside top..=bot < fb height, and dst/src are exactly one row apart so the copy is non-overlapping within the mapped framebuffer.
            unsafe {
                core::ptr::copy_nonoverlapping(src as *const u8, dst as *mut u8, row_bytes);
            }
        }
        // Clear the bottom row of the region.
        let dst = base + self.bot * row_bytes;
        clear_row_bytes(dst, row_bytes, self.bg);
    }

    /// Scroll the region down by one line (content moves down).
    pub(super) fn scroll_down(&mut self) {
        let (_, pitch, bpp, _, base) = fb_info();
        if bpp != 32 || base == 0 {
            return;
        }
        let row_bytes = pitch * FONT_H;
        let mut row = self.bot;
        while row > self.top {
            let dst = base + row * row_bytes;
            let src = base + (row - 1) * row_bytes;
            // SAFETY: same fb::info()-validated geometry; rows descend with src one row above dst, so the copy is non-overlapping within the mapped framebuffer.
            unsafe {
                core::ptr::copy_nonoverlapping(src as *const u8, dst as *mut u8, row_bytes);
            }
            row -= 1;
        }
        let dst = base + self.top * row_bytes;
        clear_row_bytes(dst, row_bytes, self.bg);
    }

    pub(super) fn erase_display(&mut self, mode: u32) {
        match mode {
            0 => {
                self.erase_line(0);
                for row in (self.cur_row + 1)..self.rows {
                    self.clear_row(row);
                }
            }
            1 => {
                self.erase_line(1);
                for row in 0..self.cur_row {
                    self.clear_row(row);
                }
            }
            _ => {
                for row in 0..self.rows {
                    self.clear_row(row);
                }
            }
        }
    }

    pub(super) fn erase_line(&mut self, mode: u32) {
        let (w, h) = (FONT_W, FONT_H);
        let x0 = match mode {
            1 => 0,
            _ => self.cur_col * w,
        };
        let x1 = match mode {
            0 | 1 => self.cols * w,
            _ => (self.cur_col + 1) * w,
        };
        let y = self.cur_row * h;
        for x in x0..x1.min(fb::width()) {
            for dy in 0..h.min(fb::height() - y) {
                fb::put_pixel_blend(x, y + dy, self.bg);
            }
        }
        if mode == 1 {
            // Classic behavior: erase up to AND including the cursor cell.
            self.draw_char_at(self.cur_row, self.cur_col, b' ');
        }
    }

    fn clear_row(&mut self, row: usize) {
        let (_, pitch, bpp, _, base) = fb_info();
        if bpp == 32 && base != 0 {
            clear_row_bytes(base + row * pitch * FONT_H, pitch * FONT_H, self.bg);
        } else {
            for x in 0..self.cols * FONT_W {
                for dy in 0..FONT_H {
                    fb::put_pixel_blend(x, row * FONT_H + dy, self.bg);
                }
            }
        }
    }

    pub(super) fn print_char(&mut self, c: u8) {
        self.draw_char_at(self.cur_row, self.cur_col, c);
        self.cur_col += 1;
        if self.cur_col >= self.cols {
            self.cur_col = 0;
            self.index();
        }
    }

    fn draw_char_at(&mut self, row: usize, col: usize, c: u8) {
        let (fg, bg) = if self.reverse {
            (self.bg, self.fg)
        } else {
            (self.fg, self.bg)
        };
        fb::draw_char(col * FONT_W, row * FONT_H, c, fg, bg);
    }

    /// Draw the cursor block at the current position (called by the kernel
    /// after console writes when the cursor is visible).
    pub fn draw_cursor(&mut self) {
        if !self.cursor_visible {
            return;
        }
        // Invert the cell by re-drawing a block glyph over the position.
        let (fg, bg) = if self.reverse {
            (self.bg, self.fg)
        } else {
            (self.fg, self.bg)
        };
        fb::draw_char(self.cur_col * FONT_W, self.cur_row * FONT_H, b' ', bg, fg);
    }

    // ── Alt-screen swap (CSI ?1049 h/l, todo v0.6 #3) ────────────────────
    //
    // One saved-normal surface is enough: alt mode is a flag, not a stack
    // (nested 1049h just re-saves the current — empty — alt surface). The
    // surface is a lazily allocated contiguous pmm run (same pattern as the
    // fb back buffer); when the allocation fails the switch is a no-op and
    // programs keep drawing on the normal screen. Cursor save/restore reuses
    // the ESC 7/8 slot (programs that mix both sequences will see the last
    // save win — documented compromise).

    /// CSI ?1049h: save the visible screen, clear it, home the cursor.
    pub(super) fn enter_alt(&mut self) {
        let (base, bytes) = match surface() {
            Some(v) => v,
            None => return,
        };
        let pa = ensure_saved(bytes);
        if pa == 0 {
            return;
        }
        // SAFETY: save surface is a pmm-owned contiguous run of `bytes`
        // length; the front buffer is fb::info()-validated of the same size;
        // non-overlapping by construction.
        unsafe {
            core::ptr::copy_nonoverlapping(base as *const u8, pa as *mut u8, bytes);
        }
        self.save_cursor();
        clear_surface(base, bytes, self.bg);
        self.cur_row = 0;
        self.cur_col = 0;
    }

    /// CSI ?1049l: restore the saved normal screen and the cursor.
    pub(super) fn exit_alt(&mut self) {
        let (base, bytes) = match surface() {
            Some(v) => v,
            None => return,
        };
        let pa = G_SAVE_PA.load(Ordering::Acquire);
        if pa == 0 {
            return;
        }
        // SAFETY: same surface contract as enter_alt.
        unsafe {
            core::ptr::copy_nonoverlapping(pa as *const u8, base as *mut u8, bytes);
        }
        self.restore_cursor();
    }
}

fn fb_info() -> (usize, usize, usize, usize, usize) {
    fb::info()
}

/// (front buffer VA, size in bytes) — None when fb is not 32bpp or absent.
fn surface() -> Option<(usize, usize)> {
    let (w, pitch, bpp, _, base) = fb_info();
    if bpp != 32 || base == 0 || w == 0 {
        return None;
    }
    Some((base, pitch * (fb::height() / FONT_H) * FONT_H))
}

/// Allocate (once) and publish the saved-normal surface. Returns its VA or
/// 0 when the contiguous allocation failed.
fn ensure_saved(bytes: usize) -> usize {
    let pages = bytes.div_ceil(crate::mm::pmm::PAGE_SIZE);
    // SAFETY: pmm::init has completed by the time a framebuffer exists;
    // alloc_n self-locks and zeroes the returned run.
    match unsafe { crate::mm::pmm::alloc_n(pages) } {
        Ok(pa) => {
            match G_SAVE_PA.compare_exchange(0, pa as usize, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => pa as usize,
                Err(existing) => {
                    // Another hart won the race; return our orphaned run.
                    // SAFETY: `pa` is a page-aligned allocation we own and
                    // never published; free revalidates the range.
                    unsafe { crate::mm::pmm::free(pa) };
                    existing
                }
            }
        }
        Err(_) => 0,
    }
}

/// Fill the whole surface with a 32bpp color.
fn clear_surface(base: usize, bytes: usize, color: u32) {
    clear_row_bytes(base, bytes, color);
}

pub(super) fn clear_row_bytes(dst: usize, len: usize, color: u32) {
    let bytes = color.to_le_bytes();
    // SAFETY: callers pass dst = base + row*pitch*FONT_H inside the fb::info()-validated framebuffer with len = pitch*FONT_H, so the byte stores stay within the mapped FB.
    unsafe {
        let d = dst as *mut u8;
        for i in 0..len {
            *d.add(i) = bytes[i & 3];
        }
    }
}

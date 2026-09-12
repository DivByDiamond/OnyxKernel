//! Pixel rendering: draw, erase, scroll, cursor, alt-screen.
use super::state::{AnsiTerm, FONT_H, FONT_W};
use crate::drivers::fb;
use core::sync::atomic::{AtomicUsize, Ordering};

static G_SAVE_PA: AtomicUsize = AtomicUsize::new(0);

impl AnsiTerm {
    pub(super) fn scroll_up(&mut self) {
        let (_, pitch, bpp, _) = paint_info();
        let base = paint_base_addr();
        if base == 0 || pitch == 0 {
            return;
        }
        if bpp != 32 && bpp != 16 {
            return;
        }
        let row_bytes = pitch * FONT_H;
        for row in self.top..self.bot {
            let dst = base + row * row_bytes;
            let src = base + (row + 1) * row_bytes;
            // SAFETY: base/pitch from validated fb geometry; rows in top..=bot < height; dst/src non-overlapping one row apart.
            unsafe {
                core::ptr::copy_nonoverlapping(src as *const u8, dst as *mut u8, row_bytes);
            }
        }
        clear_row_bytes(base + self.bot * row_bytes, row_bytes, self.bg, bpp);
    }

    pub(super) fn scroll_down(&mut self) {
        let (_, pitch, bpp, _) = paint_info();
        let base = paint_base_addr();
        if base == 0 || pitch == 0 {
            return;
        }
        if bpp != 32 && bpp != 16 {
            return;
        }
        let row_bytes = pitch * FONT_H;
        let mut row = self.bot;
        while row > self.top {
            let dst = base + row * row_bytes;
            let src = base + (row - 1) * row_bytes;
            // SAFETY: same contract as scroll_up.
            unsafe {
                core::ptr::copy_nonoverlapping(src as *const u8, dst as *mut u8, row_bytes);
            }
            row -= 1;
        }
        clear_row_bytes(base + self.top * row_bytes, row_bytes, self.bg, bpp);
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
            self.draw_char_at(self.cur_row, self.cur_col, b' ');
        }
    }

    fn clear_row(&mut self, row: usize) {
        let (_, pitch, bpp, _) = paint_info();
        let base = paint_base_addr();
        if base != 0 && (bpp == 32 || bpp == 16) {
            clear_row_bytes(base + row * pitch * FONT_H, pitch * FONT_H, self.bg, bpp);
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

    pub fn draw_cursor(&mut self) {
        if !self.cursor_visible {
            return;
        }
        let (fg, bg) = if self.reverse {
            (self.bg, self.fg)
        } else {
            (self.fg, self.bg)
        };
        fb::draw_char(self.cur_col * FONT_W, self.cur_row * FONT_H, b' ', bg, fg);
    }

    pub(super) fn draw_cursor_front(&mut self) {
        if !self.cursor_visible {
            return;
        }
        let block = if self.reverse { self.bg } else { self.fg };
        fb::draw_cursor_cell_front(self.cur_col, self.cur_row, block);
    }

    pub(super) fn enter_alt(&mut self) {
        let (base, bytes) = match surface() {
            Some(v) => v,
            None => return,
        };
        let pa = ensure_saved(bytes);
        if pa == 0 {
            return;
        }
        // SAFETY: save surface is pmm contiguous run of bytes; paint surface validated same size.
        unsafe {
            core::ptr::copy_nonoverlapping(base as *const u8, pa as *mut u8, bytes);
        }
        self.save_cursor();
        clear_surface(base, bytes, self.bg);
        self.cur_row = 0;
        self.cur_col = 0;
    }

    pub(super) fn exit_alt(&mut self) {
        let (base, bytes) = match surface() {
            Some(v) => v,
            None => return,
        };
        let pa = G_SAVE_PA.load(Ordering::Acquire);
        if pa == 0 {
            return;
        }
        // SAFETY: same contract as enter_alt.
        unsafe {
            core::ptr::copy_nonoverlapping(pa as *const u8, base as *mut u8, bytes);
        }
        self.restore_cursor();
    }
}

fn fb_info() -> (usize, usize, usize, usize, usize) {
    fb::info()
}
fn paint_info() -> (usize, usize, usize, usize) {
    let (_, pitch, bpp, height, _) = fb_info();
    (paint_base_addr(), pitch, bpp, height)
}
fn paint_base_addr() -> usize {
    fb::paint_base() as usize
}

fn surface() -> Option<(usize, usize)> {
    let (base, pitch, bpp, height) = paint_info();
    if base == 0 || pitch == 0 || (bpp != 32 && bpp != 16) {
        return None;
    }
    Some((base, pitch * (height / FONT_H) * FONT_H))
}

fn ensure_saved(bytes: usize) -> usize {
    let pages = bytes.div_ceil(crate::mm::pmm::PAGE_SIZE);
    match unsafe { crate::mm::pmm::alloc_n(pages) } {
        Ok(pa) => {
            match G_SAVE_PA.compare_exchange(0, pa as usize, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => pa as usize,
                Err(existing) => {
                    unsafe { crate::mm::pmm::free(pa) };
                    existing
                }
            }
        }
        Err(_) => 0,
    }
}

fn clear_surface(base: usize, bytes: usize, color: u32) {
    let (_, _, bpp, _) = paint_info();
    clear_row_bytes(base, bytes, color, bpp);
}

pub(super) fn clear_row_bytes(dst: usize, len: usize, color: u32, bpp: usize) {
    // SAFETY: callers pass dst = base + row*pitch*FONT_H inside validated fb; len = pitch*FONT_H.
    unsafe {
        if bpp == 16 {
            let px = fb::rgb32_to_r5g6b5(color).to_le();
            let d = dst as *mut u16;
            let words = len / 2;
            for i in 0..words {
                *d.add(i) = px;
            }
            if len & 1 != 0 {
                *(dst as *mut u8).add(len - 1) = (px & 0xFF) as u8;
            }
        } else {
            let bytes = color.to_le_bytes();
            let d = dst as *mut u8;
            for i in 0..len {
                *d.add(i) = bytes[i & 3];
            }
        }
    }
}

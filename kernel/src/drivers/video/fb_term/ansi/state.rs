//! Terminal state for the ANSI console: grid dimensions, cursor position,
//! SGR color/attribute state, scroll region and the saved-cursor slot.
//!
//! Rendering lives in [`render`](super::render), parsing in
//! [`parse`](super::parse); both operate on the fields defined here.

use crate::drivers::fb;

pub(crate) const FONT_W: usize = 8;
pub(crate) const FONT_H: usize = 16;

#[derive(PartialEq, Clone, Copy)]
pub(super) enum ParseState {
    Ground,
    Esc,
    Csi,
}

pub struct AnsiTerm {
    /// Grid dimensions in cells.
    pub cols: usize,
    pub rows: usize,
    /// Cursor position (cell coordinates).
    pub(super) cur_row: usize,
    pub(super) cur_col: usize,
    /// Current colors (SGR state).
    pub(super) fg: u32,
    pub(super) bg: u32,
    pub(super) reverse: bool,
    /// Saved cursor state.
    save_row: usize,
    save_col: usize,
    save_fg: u32,
    save_bg: u32,
    /// Scroll region (inclusive, 0-based).
    pub(super) top: usize,
    pub(super) bot: usize,
    /// Parser state.
    pub(super) state: ParseState,
    /// Numeric parameters of the active CSI sequence.
    pub(super) params: [u32; 16],
    pub(super) nparams: usize,
    /// Private-marker ('?' seen).
    pub(super) private: bool,
    /// Cursor visible.
    pub cursor_visible: bool,
}

impl AnsiTerm {
    pub const fn new() -> Self {
        Self {
            cols: 80,
            rows: 25,
            cur_row: 0,
            cur_col: 0,
            fg: fb::COL_GREEN,
            bg: fb::COL_BLACK,
            reverse: false,
            save_row: 0,
            save_col: 0,
            save_fg: fb::COL_GREEN,
            save_bg: fb::COL_BLACK,
            top: 0,
            bot: 24,
            state: ParseState::Ground,
            params: [0; 16],
            nparams: 0,
            private: false,
            cursor_visible: true,
        }
    }

    /// Recompute grid dimensions from the current framebuffer geometry and
    /// clamp the scroll region and cursor. Keeps `top <= bot` and the
    /// cursor inside the region so later `clamp(top, bot)` calls in the
    /// parser can never panic (e.g. after a hot-swap to a smaller fb).
    pub(super) fn sync_size(&mut self) {
        self.cols = (fb::width() / FONT_W).max(1);
        self.rows = (fb::height() / FONT_H).max(1);
        if self.bot >= self.rows {
            self.bot = self.rows - 1;
        }
        if self.top > self.bot {
            self.top = self.bot;
        }
        self.cur_row = self.cur_row.clamp(self.top, self.bot);
        self.cur_col = self.cur_col.min(self.cols - 1);
    }

    pub(super) fn save_cursor(&mut self) {
        self.save_row = self.cur_row;
        self.save_col = self.cur_col;
        self.save_fg = self.fg;
        self.save_bg = self.bg;
    }

    pub(super) fn restore_cursor(&mut self) {
        self.cur_row = self.save_row.min(self.rows - 1);
        self.cur_col = self.save_col.min(self.cols - 1);
        self.fg = self.save_fg;
        self.bg = self.save_bg;
    }
}

impl Default for AnsiTerm {
    fn default() -> Self {
        Self::new()
    }
}

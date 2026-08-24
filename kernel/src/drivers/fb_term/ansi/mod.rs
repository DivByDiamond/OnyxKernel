//! ANSI/VT100 escape-sequence interpreter for the kernel console.
//!
//! Full-screen terminal programs (editors like oed, monitors like osysmon)
//! drive the console with escape sequences. The kernel's fb_term provides a
//! plain character grid; this module interprets the standard CSI/ESC
//! commands and applies them to the grid, giving real cursor addressing,
//! colors, erase operations and scroll regions — enough for nano/vim/btop
//! style UIs without any userspace framebuffer access.
//!
//! Module layout (responsibility split):
//! - `mod.rs` (this file): parser state machine and terminal state
//!   (cursor position, SGR attributes, scroll region, saved cursor).
//! - [`render`]: pixel-level rendering — character drawing, erase
//!   operations, region scrolling and the software cursor block.
//!
//! Supported sequences:
//!   CSI n A/B/C/D  — cursor up/down/forward/back
//!   CSI n;m H / f  — cursor position (1-based)
//!   CSI n G        — cursor to column
//!   CSI n d        — cursor to row
//!   CSI s / u      — save/restore cursor
//!   CSI ?25 h/l    — show/hide cursor
//!   CSI n J        — erase display (0=below,1=above,2=all,3=all+scrollback)
//!   CSI n K        — erase line   (0=right,1=left,2=all)
//!   CSI n;m;r SGR  — colors/attributes (0 reset,1 bold,7 reverse,22,27,
//!                    30-37/90-97 fg, 40-47/100-107 bg, 39/49 default)
//!   CSI r          — set scroll region (top;bottom)
//!   ESC 7 / 8      — save/restore cursor (DEC)
//!   ESC M          — reverse index (scroll down inside region)
//!   ESC D          — index (scroll up inside region)
//!   ESC E          — next line

pub(crate) mod render;

use crate::drivers::fb;

pub(crate) const FONT_W: usize = 8;
pub(crate) const FONT_H: usize = 16;

pub struct AnsiTerm {
    /// Grid dimensions in cells.
    pub cols: usize,
    pub rows: usize,
    /// Cursor position (cell coordinates).
    pub cur_row: usize,
    pub cur_col: usize,
    /// Current colors (SGR state).
    pub fg: u32,
    pub bg: u32,
    pub bold: bool,
    pub reverse: bool,
    /// Saved cursor state.
    save_row: usize,
    save_col: usize,
    save_fg: u32,
    save_bg: u32,
    /// Scroll region (inclusive, 0-based).
    pub(super) top: usize,
    pub(super) bot: usize,
    /// Parser state.
    state: ParseState,
    /// Numeric parameters of the active CSI sequence.
    params: [u32; 16],
    nparams: usize,
    /// Private-marker ('?' seen).
    private: bool,
    /// Cursor visible.
    pub cursor_visible: bool,
    /// Cursor position at the previous console_cursor() call (reserved for
    /// diff-based redraw; currently write-only state).
    #[allow(dead_code)]
    last_row: usize,
    #[allow(dead_code)]
    last_col: usize,
}

#[derive(PartialEq, Clone, Copy)]
enum ParseState {
    Ground,
    Esc,
    Csi,
}

impl Default for AnsiTerm {
    fn default() -> Self {
        Self::new()
    }
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
            bold: false,
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
            last_row: 0,
            last_col: 0,
        }
    }

    fn sync_size(&mut self) {
        self.cols = (fb::width() / FONT_W).max(1);
        self.rows = (fb::height() / FONT_H).max(1);
        if self.bot >= self.rows {
            self.bot = self.rows - 1;
        }
    }

    /// Reset state (used when the console is re-initialized).
    pub fn reset(&mut self) {
        self.sync_size();
        self.cur_row = 0;
        self.cur_col = 0;
        self.fg = fb::COL_GREEN;
        self.bg = fb::COL_BLACK;
        self.bold = false;
        self.reverse = false;
        self.top = 0;
        self.bot = self.rows.saturating_sub(1);
        self.state = ParseState::Ground;
        self.nparams = 0;
        self.private = false;
        self.cursor_visible = true;
    }

    /// Feed one byte from a console write. Bytes are drawn through the
    /// fb_term writer; escape sequences update the parser and the grid.
    pub fn putc(&mut self, c: u8) {
        self.sync_size();

        match self.state {
            ParseState::Ground => match c {
                0x1b => self.state = ParseState::Esc,
                b'\n' => self.newline(),
                b'\r' => self.cur_col = 0,
                b'\t' => {
                    let next = (self.cur_col + 8) & !7;
                    self.cur_col = next.min(self.cols - 1);
                }
                0x08 => {
                    if self.cur_col > 0 {
                        self.cur_col -= 1;
                    }
                }
                0x07 => { /* BEL: ignore (no audio) */ }
                _ => self.print_char(c),
            },
            ParseState::Esc => match c {
                b'[' => {
                    self.state = ParseState::Csi;
                    self.nparams = 0;
                    self.params = [0; 16];
                    self.private = false;
                }
                b'7' => {
                    self.save_cursor();
                    self.state = ParseState::Ground;
                }
                b'8' => {
                    self.restore_cursor();
                    self.state = ParseState::Ground;
                }
                b'M' => {
                    self.reverse_index();
                    self.state = ParseState::Ground;
                }
                b'D' => {
                    self.index();
                    self.state = ParseState::Ground;
                }
                b'E' => {
                    self.cur_col = 0;
                    self.index();
                    self.state = ParseState::Ground;
                }
                b'(' | b')' => {
                    // Charset designation — swallow next byte.
                    self.state = ParseState::Ground;
                }
                b']' => {
                    // OSC — ignore until BEL/ST (simplified: swallow).
                    self.state = ParseState::Ground;
                }
                _ => self.state = ParseState::Ground,
            },
            ParseState::Csi => self.csi_byte(c),
        }
    }

    fn csi_byte(&mut self, c: u8) {
        if c.is_ascii_digit() {
            let p = &mut self.params[self.nparams.min(15)];
            *p = p.saturating_mul(10).saturating_add((c - b'0') as u32);
        } else if c == b';' {
            if self.nparams < 15 {
                self.nparams += 1;
            }
        } else if c == b'?' {
            self.private = true;
        } else if (0x40..=0x7e).contains(&c) {
            self.dispatch_csi(c);
            self.state = ParseState::Ground;
        }
        // Other intermediates (0x20-0x2f) ignored.
    }

    fn p(&self, idx: usize, default: u32) -> u32 {
        if idx < self.nparams && self.params[idx] != 0 {
            self.params[idx]
        } else {
            default
        }
    }

    fn dispatch_csi(&mut self, cmd: u8) {
        match cmd {
            b'A' => {
                let n = self.p(0, 1) as usize;
                self.cur_row = self.cur_row.saturating_sub(n).max(self.top);
            }
            b'B' => {
                let n = self.p(0, 1) as usize;
                self.cur_row = (self.cur_row + n).min(self.bot);
            }
            b'C' => {
                let n = self.p(0, 1) as usize;
                self.cur_col = (self.cur_col + n).min(self.cols - 1);
            }
            b'D' => {
                let n = self.p(0, 1) as usize;
                self.cur_col = self.cur_col.saturating_sub(n);
            }
            b'G' | b'`' => {
                let col = self.p(0, 1) as usize;
                self.cur_col = (col.saturating_sub(1)).min(self.cols - 1);
            }
            b'd' => {
                let row = self.p(0, 1) as usize;
                let row = (row.saturating_sub(1)).min(self.rows - 1);
                self.cur_row = row.clamp(self.top, self.bot);
            }
            b'H' | b'f' => {
                let row = self.p(0, 1) as usize;
                let col = self.p(1, 1) as usize;
                let row = (row.saturating_sub(1)).min(self.rows - 1);
                let col = (col.saturating_sub(1)).min(self.cols - 1);
                self.cur_row = row.clamp(self.top, self.bot);
                self.cur_col = col;
            }
            b'J' => {
                let mode = if self.nparams > 0 { self.params[0] } else { 0 };
                self.erase_display(mode);
            }
            b'K' => {
                let mode = if self.nparams > 0 { self.params[0] } else { 0 };
                self.erase_line(mode);
            }
            b's' => self.save_cursor(),
            b'u' => self.restore_cursor(),
            b'h' | b'l' => {
                if self.private && self.nparams >= 1 && self.params[0] == 25 {
                    self.cursor_visible = cmd == b'h';
                }
                // 1049 (alt screen), 2004 (bracketed paste): accepted, no-op.
            }
            b'm' => self.sgr(),
            b'r' => {
                let top = self.p(0, 1) as usize;
                let bot = self.p(1, self.rows as u32) as usize;
                let top = (top.saturating_sub(1)).min(self.rows - 1);
                let bot = (bot.saturating_sub(1)).min(self.rows - 1);
                if top < bot {
                    self.top = top;
                    self.bot = bot;
                    self.cur_row = top;
                    self.cur_col = 0;
                }
            }
            b'S' => {
                let n = self.p(0, 1) as usize;
                for _ in 0..n {
                    self.scroll_up();
                }
            }
            b'T' => {
                let n = self.p(0, 1) as usize;
                for _ in 0..n {
                    self.reverse_index();
                }
            }
            _ => { /* unsupported: ignore */ }
        }
    }

    fn sgr(&mut self) {
        if self.nparams == 0 {
            self.params[0] = 0;
            self.nparams = 1;
        }
        let mut i = 0;
        while i < self.nparams.max(1) {
            let p = self.params[i.min(15)];
            match p {
                0 => {
                    self.fg = fb::COL_GREEN;
                    self.bg = fb::COL_BLACK;
                    self.bold = false;
                    self.reverse = false;
                }
                1 => self.bold = true,
                7 => self.reverse = true,
                22 => self.bold = false,
                27 => self.reverse = false,
                30..=37 => self.fg = sgr_color(p - 30, false),
                90..=97 => self.fg = sgr_color(p - 90, true),
                40..=47 => self.bg = sgr_color(p - 40, false),
                100..=107 => self.bg = sgr_color(p - 100, true),
                39 => self.fg = fb::COL_GREEN,
                49 => self.bg = fb::COL_BLACK,
                _ => {}
            }
            i += 1;
        }
    }

    fn save_cursor(&mut self) {
        self.save_row = self.cur_row;
        self.save_col = self.cur_col;
        self.save_fg = self.fg;
        self.save_bg = self.bg;
    }

    fn restore_cursor(&mut self) {
        self.cur_row = self.save_row.min(self.rows - 1);
        self.cur_col = self.save_col.min(self.cols - 1);
        self.fg = self.save_fg;
        self.bg = self.save_bg;
    }

    /// Move the cursor down one line, scrolling the region up when at its
    /// bottom edge.
    fn index(&mut self) {
        if self.cur_row == self.bot {
            self.scroll_up();
        } else if self.cur_row < self.rows - 1 {
            self.cur_row += 1;
        }
    }

    /// Move the cursor up one line, scrolling the region down when at its
    /// top edge.
    fn reverse_index(&mut self) {
        if self.cur_row == self.top {
            self.scroll_down();
        } else if self.cur_row > 0 {
            self.cur_row -= 1;
        }
    }

    fn newline(&mut self) {
        self.index();
    }
}

fn sgr_color(idx: u32, bright: bool) -> u32 {
    let base = if bright { 8 } else { 0 };
    match idx + base {
        0 => fb::COL_BLACK,
        1 => fb::COL_RED,
        2 => fb::COL_GREEN,
        3 => fb::COL_YELLOW,
        4 => fb::COL_BLUE,
        5 => fb::COL_MAGENTA,
        6 => fb::COL_CYAN,
        _ => fb::COL_WHITE,
    }
}

/// Global console state used by the sys_write path.
pub(crate) static mut G_ANSI: AnsiTerm = AnsiTerm::new();

/// Write a byte through the ANSI interpreter.
pub fn console_putc(c: u8) {
    unsafe {
        let t = &raw mut G_ANSI;
        (*t).putc(c);
    }
}

/// Write a string through the ANSI interpreter.
pub fn console_puts(s: &str) {
    for &b in s.as_bytes() {
        console_putc(b);
    }
}

/// Refresh the cursor after a console operation.
pub fn console_cursor() {
    unsafe {
        let t = &raw mut G_ANSI;
        (*t).draw_cursor();
    }
}

/// Reinitialize after mode/modebox change.
pub fn console_reset() {
    unsafe {
        let t = &raw mut G_ANSI;
        (*t).reset();
    }
}

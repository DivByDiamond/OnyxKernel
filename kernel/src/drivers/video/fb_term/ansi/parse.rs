//! Parser state machine for the ANSI console: byte feeding, CSI/ESC
//! sequence interpretation and SGR attribute handling.
//!
//! Terminal state ([`AnsiTerm`](super::state::AnsiTerm)) and pixel-level
//! rendering ([`render`](super::render)) live in sibling modules.

use super::state::{AnsiTerm, ParseState};
use crate::drivers::fb;

impl AnsiTerm {
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
                    self.reverse = false;
                }
                7 => self.reverse = true,
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

    /// Move the cursor down one line, scrolling the region up when at its
    /// bottom edge.
    pub(super) fn index(&mut self) {
        if self.cur_row == self.bot {
            self.scroll_up();
        } else if self.cur_row < self.rows - 1 {
            self.cur_row += 1;
        }
    }

    /// Move the cursor up one line, scrolling the region down when at its
    /// top edge.
    pub(super) fn reverse_index(&mut self) {
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

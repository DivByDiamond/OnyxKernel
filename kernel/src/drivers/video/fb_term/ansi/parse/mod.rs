//! ANSI parser: byte feed, CSI dispatch and SGR.
mod sgr;

use super::state::{AnsiTerm, ParseState};

impl AnsiTerm {
    pub fn putc(&mut self, c: u8) {
        self.sync_size();
        match self.state {
            ParseState::Ground => match c {
                0x1b => self.state = ParseState::Esc,
                b'\n' => self.newline(),
                b'\r' => self.cur_col = 0,
                b'\t' => self.cur_col = ((self.cur_col + 8) & !7).min(self.cols - 1),
                0x08 => self.cur_col = self.cur_col.saturating_sub(1),
                0x07 => {}
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
                b'(' | b')' => self.state = ParseState::Ground,
                b']' => self.state = ParseState::Ground,
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
    }

    fn p(&self, idx: usize, default: u32) -> u32 {
        if idx <= self.nparams && idx < 16 && self.params[idx] != 0 {
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
                self.cur_col = col.saturating_sub(1).min(self.cols - 1);
            }
            b'd' => {
                let row = self.p(0, 1) as usize;
                let row = row.saturating_sub(1).min(self.rows - 1);
                self.cur_row = row.clamp(self.top, self.bot);
            }
            b'H' | b'f' => {
                let row = self.p(0, 1) as usize;
                let col = self.p(1, 1) as usize;
                let row = row.saturating_sub(1).min(self.rows - 1);
                let col = col.saturating_sub(1).min(self.cols - 1);
                self.cur_row = row.clamp(self.top, self.bot);
                self.cur_col = col;
            }
            b'J' => {
                let mode = self.params[0];
                self.erase_display(mode);
            }
            b'K' => {
                let mode = self.params[0];
                self.erase_line(mode);
            }
            b's' => self.save_cursor(),
            b'u' => self.restore_cursor(),
            b'h' | b'l' => {
                if self.private {
                    self.set_private_mode(self.params[0], cmd == b'h');
                }
            }
            b'm' => self.sgr(),
            b'r' => {
                let top = self.p(0, 1) as usize;
                let bot = self.p(1, self.rows as u32) as usize;
                let top = top.saturating_sub(1).min(self.rows - 1);
                let bot = bot.saturating_sub(1).min(self.rows - 1);
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
            _ => {}
        }
    }

    pub(super) fn index(&mut self) {
        if self.cur_row == self.bot {
            self.scroll_up();
        } else if self.cur_row < self.rows - 1 {
            self.cur_row += 1;
        }
    }
    pub(super) fn reverse_index(&mut self) {
        if self.cur_row == self.top {
            self.scroll_down();
        } else if self.cur_row > 0 {
            self.cur_row -= 1;
        }
    }
    fn newline(&mut self) {
        self.index();
        self.cur_col = 0;
    }
}

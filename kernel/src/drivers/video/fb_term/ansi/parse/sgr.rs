//! SGR (CSI m) handling: 8/16-color, 256-color and direct-color selection.
use super::super::state::AnsiTerm;
use crate::drivers::fb;

const ANSI: [u32; 8] = [
    fb::COL_BLACK,
    fb::COL_RED,
    fb::COL_GREEN,
    fb::COL_YELLOW,
    fb::COL_BLUE,
    fb::COL_MAGENTA,
    fb::COL_CYAN,
    fb::COL_WHITE,
];
const BRIGHT: [u32; 8] = [
    0x55_55_55, 0xFF_55_55, 0x55_FF_55, 0xFF_FF_55, 0x55_55_FF, 0xFF_55_FF, 0x55_FF_FF, 0xFF_FF_FF,
];

pub(super) const DEFAULT_FG: u32 = fb::COL_WHITE;
pub(super) const DEFAULT_BG: u32 = fb::COL_BLACK;

impl AnsiTerm {
    pub(super) fn sgr(&mut self) {
        let n = (self.nparams + 1).min(16);
        let mut i = 0;
        while i < n {
            match self.params[i] {
                0 => {
                    self.fg = DEFAULT_FG;
                    self.bg = DEFAULT_BG;
                    self.reverse = false;
                }
                7 => self.reverse = true,
                27 => self.reverse = false,
                30..=37 => self.fg = color(self.params[i] - 30, false),
                39 => self.fg = DEFAULT_FG,
                40..=47 => self.bg = color(self.params[i] - 40, false),
                49 => self.bg = DEFAULT_BG,
                90..=97 => self.fg = color(self.params[i] - 90, true),
                100..=107 => self.bg = color(self.params[i] - 100, true),
                38 | 48 => match self.extended_color(i, n) {
                    Some((value, used)) => {
                        if self.params[i] == 38 {
                            self.fg = value;
                        } else {
                            self.bg = value;
                        }
                        i += used;
                    }
                    None => break,
                },
                _ => {}
            }
            i += 1;
        }
    }

    fn extended_color(&self, start: usize, n: usize) -> Option<(u32, usize)> {
        let kind = self.params.get(start + 1).copied()?;
        match kind {
            5 if start + 2 < n => Some((palette256(self.params[start + 2]), 2)),
            2 if start + 4 < n => {
                let rgb = self.params[start + 2..start + 5]
                    .iter()
                    .fold(0u32, |acc, &v| (acc << 8) | v.min(255));
                Some((rgb, 4))
            }
            _ => None,
        }
    }
}

fn color(idx: u32, bright: bool) -> u32 {
    let i = (idx & 7) as usize;
    if bright { BRIGHT[i] } else { ANSI[i] }
}

fn palette256(index: u32) -> u32 {
    match index {
        0..=7 => ANSI[index as usize],
        8..=15 => BRIGHT[(index - 8) as usize],
        16..=231 => {
            let v = index - 16;
            rgb((v / 36) * 51, ((v / 6) % 6) * 51, (v % 6) * 51)
        }
        232..=255 => {
            let gray = 8 + (index - 232) * 10;
            rgb(gray, gray, gray)
        }
        _ => fb::COL_WHITE,
    }
}

fn rgb(r: u32, g: u32, b: u32) -> u32 {
    let r = quantize_cube_channel(r);
    let g = quantize_cube_channel(g);
    let b = quantize_cube_channel(b);
    (r << 16) | (g << 8) | b
}

fn quantize_cube_channel(value: u32) -> u32 {
    if value == 0 {
        0
    } else {
        (value * 255 / 255).min(255)
    }
}

#[cfg(test)]
mod tests {
    use super::{AnsiTerm, DEFAULT_BG, DEFAULT_FG};

    fn feed(term: &mut AnsiTerm, bytes: &[u8]) {
        for &b in bytes {
            term.putc(b);
        }
    }

    #[test]
    fn resets_to_normal_console_colors() {
        let mut term = AnsiTerm::new();
        feed(&mut term, b"\x1b[0m");
        assert_eq!(term.fg, DEFAULT_FG);
        assert_eq!(term.bg, DEFAULT_BG);
    }

    #[test]
    fn maps_bright_and_basic_colors() {
        let mut term = AnsiTerm::new();
        feed(&mut term, b"\x1b[1;36m");
        assert_eq!(term.fg, 0x00_FF_FF);
        feed(&mut term, b"\x1b[1;101m");
        assert_eq!(term.bg, 0xFF_55_55);
    }

    #[test]
    fn maps_extended_and_true_color() {
        let mut term = AnsiTerm::new();
        feed(&mut term, b"\x1b[38;5;196m");
        assert_eq!(term.fg, 0xFF_00_00);
        feed(&mut term, b"\x1b[48;2;16;32;64m");
        assert_eq!(term.bg, 0x10_20_40);
    }
}

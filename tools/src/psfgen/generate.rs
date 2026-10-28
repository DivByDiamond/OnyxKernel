use crate::fontdata::*;

const PSF1_MAGIC: u16 = 0x0436;
const GLYPHS: usize = 256;
const GLYPH_H: usize = 16;

/// Blank cell: rendered for space, not the "unknown glyph" placeholder box.
/// A solid box in place of every space made the console unreadable — it
/// looked like screen corruption rather than whitespace (found while
/// verifying the blank-console timer/font fix, 2026-09-12).
const BLANK: [u8; 16] = [0; 16];

fn glyph_bitmap(c: u8) -> &'static [u8; 16] {
    match c {
        b'A'..=b'Z' => &ALPHA_UPPER[(c - b'A') as usize],
        b'a'..=b'z' => &ALPHA_LOWER[(c - b'a') as usize],
        b'0'..=b'9' => &DIGITS[(c - b'0') as usize],
        b' ' => &BLANK,
        // Full 7-bit ASCII coverage (2026-09-12): 0x00-0x1F (control codes)
        // and 0x7F (DEL) are non-printable by definition — the ANSI parser
        // (fb_term/ansi/parse.rs) already intercepts the ones with real
        // meaning (\n \r \t ESC) before they ever reach the glyph draw
        // path, so this only fires for stray/unhandled control bytes. A
        // real terminal renders those as nothing, not a visible mark, so
        // they get the same blank cell as space rather than the "unknown
        // glyph" placeholder box (which stays reserved for genuinely
        // unmapped bytes — extended/non-ASCII, ≥ 0x80).
        0x00..=0x1F | 0x7F => &BLANK,
        _ => punct_glyph(c).unwrap_or(&GLYPH_DEFAULT),
    }
}

fn unicode_table() -> Vec<u8> {
    let mut table = Vec::new();
    for cp in 0..GLYPHS {
        table.extend_from_slice(&(cp as u16).to_le_bytes());
        table.extend_from_slice(&0xFFFFu16.to_le_bytes());
    }
    table
}

pub fn psf1() -> Vec<u8> {
    let charsize = GLYPH_H as u32;
    let ut = unicode_table();
    let mode: u8 = 0x02;
    let total_size = 4 + GLYPHS * GLYPH_H + ut.len();
    let mut buf = Vec::with_capacity(total_size);
    buf.extend_from_slice(&PSF1_MAGIC.to_le_bytes());
    buf.push(mode);
    buf.push(charsize as u8);
    for c in 0..GLYPHS {
        buf.extend_from_slice(glyph_bitmap(c as u8));
    }
    buf.extend_from_slice(&ut);
    assert_eq!(
        buf.len(),
        total_size,
        "psf1: generated buffer size drifted from the declared header size"
    );
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alphanumeric_metrics_share_baseline_and_leave_interline_space() {
        let font = psf1();
        for c in b'0'..=b'z' {
            if !c.is_ascii_alphanumeric() {
                continue;
            }
            let glyph = &font[4 + c as usize * 16..4 + (c as usize + 1) * 16];
            assert_eq!(&glyph[..2], &[0, 0], "top bearing of {}", c as char);
            assert_eq!(glyph[15], 0, "line gap of {}", c as char);
            if !b"gjpqy".contains(&c) {
                assert_eq!(&glyph[13..], &[0, 0, 0], "baseline of {}", c as char);
                assert_ne!(glyph[12], 0, "baseline must be visible for {}", c as char);
            } else {
                assert_ne!(glyph[14], 0, "descender of {}", c as char);
            }
        }
        for c in b'0'..=b'9' {
            let g = glyph_bitmap(c);
            assert_ne!(g[2], 0);
            assert!(g[2..=12].iter().all(|r| *r != 0));
        }
        for c in b"acemnorsuvwxz" {
            assert_eq!(
                &glyph_bitmap(*c)[..5],
                &[0; 5],
                "x-height of {}",
                *c as char
            );
        }
    }

    #[test]
    fn generated_psf_keeps_ascii_mapping_and_blank_controls() {
        let font = psf1();
        assert_eq!(&font[..4], &[0x36, 0x04, 0x02, 16]);
        assert_eq!(font.len(), 4 + 256 * 16 + 256 * 4);
        for c in (0..=32).chain(core::iter::once(127)) {
            assert_eq!(&font[4 + c * 16..4 + (c + 1) * 16], &[0; 16]);
        }
        for c in 0..256 {
            assert_eq!(&font[4100 + c * 4..4104 + c * 4], &[c as u8, 0, 255, 255]);
        }
        assert_ne!(glyph_bitmap(b'0'), glyph_bitmap(b'O'));
        assert_ne!(glyph_bitmap(b'1'), glyph_bitmap(b'l'));
    }
}

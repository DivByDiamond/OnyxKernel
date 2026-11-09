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

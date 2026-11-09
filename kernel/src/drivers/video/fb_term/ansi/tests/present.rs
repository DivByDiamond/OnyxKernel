//! Present-time behaviour: cursor stamping on the front frame only, erase
//! operations, and region-present bounds.
use super::super::{AnsiTerm, G_ANSI, console_cursor, console_putc, present::present_locked};
use super::{H, LOCK, STRIDE, back_ptr, install};
use crate::drivers::fb;

#[test]
fn present_publishes_frame_and_cursor_stamps_front() {
    let _g = LOCK.lock().unwrap();
    let mut s = install();
    fb::test_publish_back(back_ptr(&s));
    let front_before = s.front.clone();

    // Full redraw with the cursor hidden (vim hides it via ESC[?25l
    // while repainting), so the presented frame is byte-exact back.
    for b in "\x1b[2J\x1b[?25l\x1b[1;1Hhello world".bytes() {
        console_putc(b);
    }
    assert_eq!(
        s.front, front_before,
        "paint leaked to front before present"
    );

    // Present (caller holds the console lock contract in tests).
    unsafe {
        present_locked();
    }

    // Invariant 3: the finished frame is now visible, byte-exact.
    assert_eq!(s.front, s.back, "present did not publish the back buffer");

    // Invariant 4: double-buffered console_cursor paints nothing.
    let t = &raw mut G_ANSI;
    unsafe {
        (*t).cur_row = 1;
        (*t).cur_col = 2;
    }
    console_cursor();
    assert_eq!(
        s.front, s.back,
        "console_cursor painted in double-buffer mode"
    );

    // Invariant 5: present-time cursor stamps ONLY the front frame.
    unsafe {
        (*t).cursor_visible = true;
        (*t).draw_cursor_front();
    }
    let cell = |buf: &[u8], row: usize, col: usize| {
        let x = col * 8;
        let y = row * 16;
        let off = y * STRIDE + x * 4;
        u32::from_le_bytes([buf[off], buf[off + 1], buf[off + 2], buf[off + 3]])
    };
    assert_ne!(
        cell(&s.front, 1, 2),
        cell(&s.back, 1, 2),
        "cursor block missing from the presented frame"
    );
    // A pixel far from the cursor cell still matches the back buffer.
    assert_eq!(cell(&s.front, 3, 5), cell(&s.back, 3, 5));
}

#[test]
fn erase_entire_line_clears_both_sides_of_cursor() {
    let _g = LOCK.lock().unwrap();
    let mut s = install();
    s.back.fill(255);
    fb::test_publish_back(back_ptr(&s));
    unsafe {
        G_ANSI = AnsiTerm::new();
    }
    for b in b"\x1b[2;6H\x1b[2K" {
        console_putc(*b);
    }
    let line = &s.back[16 * STRIDE..32 * STRIDE];
    assert!(
        line.iter().all(|b| *b == 0),
        "EL 2 left stale cells on the row"
    );
    assert!(s.back[..16 * STRIDE].iter().all(|b| *b == 255));
}

#[test]
fn erase_start_of_line_keeps_cells_right_of_cursor() {
    let _g = LOCK.lock().unwrap();
    let mut s = install();
    s.back.fill(255);
    fb::test_publish_back(back_ptr(&s));
    unsafe {
        G_ANSI = AnsiTerm::new();
    }
    for b in b"\x1b[2;6H\x1b[1K" {
        console_putc(*b);
    }
    for y in 16..32 {
        let row = &s.back[y * STRIDE..(y + 1) * STRIDE];
        assert!(row[..6 * 8 * 4].iter().all(|b| *b == 0));
        assert!(
            row[6 * 8 * 4..].iter().all(|b| *b == 255),
            "EL 1 erased cells after cursor"
        );
    }
}

#[test]
fn region_present_does_not_copy_pixels_outside_console() {
    let _g = LOCK.lock().unwrap();
    let mut s = install();
    s.back.fill(77);
    let before = s.front.clone();
    fb::test_publish_back(back_ptr(&s));
    fb::present_region(8, 16);
    for y in 0..H {
        for x in 0..STRIDE {
            let expected = if y < 16 && x < 8 * 4 {
                77
            } else {
                before[y * STRIDE + x]
            };
            assert_eq!(s.front[y * STRIDE + x], expected);
        }
    }
}

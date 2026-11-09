use super::{G_ANSI, console_cursor, console_putc};
use crate::drivers::fb;
use crate::drivers::video::fb_term::ansi::present::present_locked;

/// Serializes tests that mutate the global fb/ANSI state.
static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

const W: usize = 160;
const H: usize = 64;
const STRIDE: usize = W * 4;

struct Surfaces {
    front: Vec<u8>,
    back: Vec<u8>,
}

impl Drop for Surfaces {
    fn drop(&mut self) {
        fb::test_clear_back();
        fb::test_uninstall_front();
    }
}

fn install() -> Surfaces {
    // Sentinel pattern so any front-buffer write is detectable.
    let mut front = vec![0u8; STRIDE * H];
    for (i, b) in front.iter_mut().enumerate() {
        *b = (i % 251) as u8;
    }
    let back = vec![0u8; STRIDE * H];
    // SAFETY: test-only front installation; the surface outlives the test.
    unsafe {
        fb::test_install_front(front.as_mut_ptr(), W, H, STRIDE, 32);
    }
    Surfaces { front, back }
}

fn back_ptr(s: &Surfaces) -> usize {
    s.back.as_ptr() as usize
}

#[test]
fn output_goes_to_back_buffer_never_to_front() {
    let _g = LOCK.lock().unwrap();
    let mut s = install();
    fb::test_publish_back(back_ptr(&s));
    let front_before = s.front.clone();

    // A full-screen TUI redraw (cursor hidden, like vim during redraw):
    // clear + move + text + statusline.
    for b in "\x1b[2J\x1b[?25l\x1b[1;1Hhello\x1b[4;1H [NORMAL] test ".bytes() {
        console_putc(b);
    }

    // Invariant 1: the visible screen was NOT touched mid-redraw.
    assert_eq!(
        s.front, front_before,
        "console output leaked to the visible front buffer"
    );
    unsafe {
        present_locked();
    }
    assert_eq!(
        s.front, s.back,
        "frame did not round-trip through the back buffer"
    );
    assert_ne!(
        s.front, front_before,
        "presented frame identical to sentinel"
    );
}

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
fn direct_mode_still_paints_immediately() {
    let _g = LOCK.lock().unwrap();
    let mut s = install();
    // No back buffer published (direct/legacy mode).
    assert!(!fb::double_buffered());
    let front_before = s.front.clone();

    for b in "\x1b[2J\x1b[1;1Hlegacy".bytes() {
        console_putc(b);
    }

    // Legacy behavior preserved: paint lands on the visible surface.
    assert_ne!(s.front, front_before, "direct mode no longer paints");
}
#[test]
fn erase_entire_line_clears_both_sides_of_cursor() {
    let _g = LOCK.lock().unwrap();
    let mut s = install();
    s.back.fill(255);
    fb::test_publish_back(back_ptr(&s));
    unsafe {
        G_ANSI = super::AnsiTerm::new();
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
        G_ANSI = super::AnsiTerm::new();
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

#[test]
fn scaled_console_uses_full_screen_and_only_publishes_dirty_glyphs() {
    let _lock = LOCK.lock().unwrap();
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            crate::drivers::fb_term::set_serial_mirror(false);
            fb::test_clear_back();
            fb::test_uninstall_front();
        }
    }
    let _reset = Reset;
    // A valid static PSF1 font makes this a glyph test too. Host tests do
    // not execute the boot-time VFS font loader; its default glyph is blank.
    static FONT: [u8; 4 + 256 * 16] = {
        let mut data = [0; 4 + 256 * 16];
        data[0] = 0x36;
        data[1] = 0x04;
        data[3] = 16;
        let mut glyph = 33;
        while glyph < 127 {
            let mut row = 0;
            while row < 16 {
                data[4 + glyph * 16 + row] = 0x42;
                row += 1;
            }
            glyph += 1;
        }
        data
    };
    unsafe {
        crate::font::init(&FONT).unwrap();
    }
    crate::drivers::fb_term::set_serial_mirror(true);
    for (width, height, cols, rows) in
        [(1920, 1080, 80, 24), (640, 480, 80, 24), (320, 200, 40, 12)]
    {
        let mut front = vec![0x1234u16; width * height];
        let mut back = vec![0u16; width * height];
        unsafe {
            fb::test_install_front(front.as_mut_ptr().cast(), width, height, width * 2, 16);
            fb::test_publish_back(back.as_mut_ptr() as usize);
            G_ANSI = super::AnsiTerm::new();
        }
        let output = format!("\x1b[2J\x1b[?25l\x1b[{};{}HFOOT", rows, cols - 5);
        for byte in output.bytes() {
            console_putc(byte);
        }
        present_locked();
        let view = super::viewport::Viewport::fit(width, height, cols * 8, rows * 16).unwrap();
        let footer = view.mapped(super::viewport::Rect::new(
            (cols - 6) * 8,
            (rows - 1) * 16,
            (cols - 2) * 8,
            rows * 16,
        ));
        assert!(
            (footer.y0..footer.y1)
                .any(|y| (footer.x0..footer.x1).any(|x| front[y * width + x] != 0)),
            "last-row TUI footer was not rendered"
        );
        // Every visible pixel must be the expected nearest logical pixel,
        // while letterbox pixels are black. Native geometry stays unchanged.
        assert_eq!((fb::width(), fb::height()), (width, height));
        for y in 0..height {
            for x in 0..width {
                let expected = if x >= view.x
                    && x < view.x + view.width
                    && y >= view.y
                    && y < view.y + view.height
                {
                    let sx = (x - view.x) * view.logical_width / view.width;
                    let sy = (y - view.y) * view.logical_height / view.height;
                    back[sy * width + sx]
                } else {
                    0
                };
                assert_eq!(front[y * width + x], expected, "scaled pixel {x},{y}");
            }
        }
        front.fill(0x1234);
        for byte in b"\x1b[2;2HA" {
            console_putc(*byte);
        }
        present_locked();
        let published = front.iter().filter(|&&pixel| pixel != 0x1234).count();
        assert!(published <= 1100, "one glyph published {published} pixels");
    }
}

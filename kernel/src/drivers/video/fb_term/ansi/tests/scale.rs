//! Scaled console: nearest-neighbour geometry, letterboxing, and dirty-glyph
//! publishing only.
use super::super::{AnsiTerm, G_ANSI, console_putc, present::present_locked, viewport};
use super::LOCK;
use crate::drivers::fb;

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
            G_ANSI = AnsiTerm::new();
        }
        let output = format!("\x1b[2J\x1b[?25l\x1b[{};{}HFOOT", rows, cols - 5);
        for byte in output.bytes() {
            console_putc(byte);
        }
        present_locked();
        let view = viewport::Viewport::fit(width, height, cols * 8, rows * 16).unwrap();
        let footer = view.mapped(viewport::Rect::new(
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

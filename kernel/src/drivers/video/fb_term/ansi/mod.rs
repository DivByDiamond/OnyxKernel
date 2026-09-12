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
//! - [`state`]: terminal state — cursor position, SGR attributes, scroll
//!   region, saved cursor.
//! - [`parse`]: parser state machine — byte feed, CSI dispatch, SGR.
//! - [`render`]: pixel-level rendering — character drawing, erase
//!   operations, region scrolling and the software cursor block.
//! - [`present`]: frame lifecycle — dirty tracking, present tick, double
//!   buffer publishing and cursor stamping (TUI anti-flicker).
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
//!   CSI n;m;r SGR  — colors/attributes (0 reset,7 reverse,
//!                    30-37/90-97 fg, 40-47/100-107 bg, 39/49 default)
//!   CSI r          — set scroll region (top;bottom)
//!   ESC 7 / 8      — save/restore cursor (DEC)
//!   ESC M          — reverse index (scroll down inside region)
//!   ESC D          — index (scroll up inside region)
//!   ESC E          — next line

pub(crate) mod parse;
pub(crate) mod present;
pub(crate) mod render;
pub(crate) mod state;

pub use state::AnsiTerm;

pub use present::{
    PRESENT_FORCE_TICKS, PRESENT_IDLE_TICKS, console_cursor, console_putc, present_now,
    present_tick,
};

/// Global console state used by the sys_write path.
pub(crate) static mut G_ANSI: AnsiTerm = AnsiTerm::new();

/// Host integration test for the anti-flicker pipeline: drives the REAL
/// console write path (console_putc → ANSI parser → paint → present) against
/// host-allocated front/back surfaces and asserts the core invariants that
/// fix the vim/otop flicker:
///   1. console output NEVER touches the visible front buffer (a TUI
///      `ESC[2J` + repaint must not flash intermediate states on screen);
///   2. output lands in the back buffer (the paint surface);
///   3. fb::present() atomically publishes the painted frame front-ward;
///   4. the double-buffered cursor (console_cursor) paints nothing itself;
///   5. the present-time cursor stamps the FRONT frame only, so it never
///      bakes into the frame content (next present erases it for free);
///   6. direct-mode fallback (no back buffer) still paints (legacy).
#[cfg(test)]
mod pipeline_tests {
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
}

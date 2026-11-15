//! Double-buffered paint pipeline: console output must never leak to the
//! visible front buffer mid-redraw, and direct mode (no back buffer) still
//! paints immediately.
use super::super::{console_putc, present::present_locked};
use super::{LOCK, back_ptr, install};
use crate::drivers::fb;

#[test]
fn output_goes_to_back_buffer_never_to_front() {
    let _g = LOCK.lock().unwrap();
    let s = install();
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
    present_locked();
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
fn direct_mode_still_paints_immediately() {
    let _g = LOCK.lock().unwrap();
    let s = install();
    // No back buffer published (direct/legacy mode).
    assert!(!fb::double_buffered());
    let front_before = s.front.clone();

    for b in "\x1b[2J\x1b[1;1Hlegacy".bytes() {
        console_putc(b);
    }

    // Legacy behavior preserved: paint lands on the visible surface.
    assert_ne!(s.front, front_before, "direct mode no longer paints");
}

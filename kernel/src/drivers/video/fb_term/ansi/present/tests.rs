use super::super::state::AnsiTerm;
use super::{PRESENT_FORCE_TICKS, PRESENT_IDLE_TICKS, should_present};

#[test]
fn clean_frame_is_never_presented() {
    assert!(!should_present(false, 0, 0));
    assert!(!should_present(false, 1_000, 1_000));
}

#[test]
fn fresh_dirty_frame_waits_for_idle() {
    // Just-dirtied frame: neither idle nor force threshold met.
    assert!(!should_present(true, 0, 0));
    assert!(!should_present(true, PRESENT_IDLE_TICKS - 1, 0));
}

#[test]
fn quiet_frame_is_presented() {
    // Console idle for the debounce window: present (key/redraw done).
    assert!(should_present(true, PRESENT_IDLE_TICKS, 0));
    assert!(should_present(true, PRESENT_IDLE_TICKS + 100, 0));
}

#[test]
fn continuously_dirty_frame_is_force_presented() {
    // Continuous output stream: force a present at the latency cap
    // even though the console never went idle.
    assert!(!should_present(true, 0, PRESENT_FORCE_TICKS - 1));
    assert!(should_present(true, 0, PRESENT_FORCE_TICKS));
}

#[test]
fn thresholds_are_latency_bounded() {
    // Documented latency budget at the 100 Hz tick: 20 ms idle debounce,
    // 150 ms worst-case forced latency.
    assert_eq!(PRESENT_IDLE_TICKS * 10, 20);
    assert_eq!(PRESENT_FORCE_TICKS * 10, 150);
}

#[test]
fn dec_private_modes_parse_without_separator() {
    // ESC[?25l must hide the cursor even though the canonical
    // single-param form "?25l" has no ';' (nparams stays 0) — vim/otop
    // hide the cursor via ?25l during full redraws, so an ignored
    // sequence puts a cursor block into every intermediate frame
    // (the second artifact of the reported flicker).
    let mut t = AnsiTerm::new();
    assert!(t.cursor_visible);
    for b in b"\x1b[?25l" {
        t.putc(*b);
    }
    assert!(!t.cursor_visible, "ESC[?25l without ';' was ignored");
    for b in b"\x1b[?25h" {
        t.putc(*b);
    }
    assert!(t.cursor_visible, "ESC[?25h without ';' was ignored");
    // Multi-param form applies its first parameter (pre-existing
    // single-param semantics): ?1049;... dispatches mode 1049 only and
    // must not panic without a framebuffer.
    for b in b"\x1b[?1049;25l" {
        t.putc(*b);
    }
    assert!(t.cursor_visible);
}

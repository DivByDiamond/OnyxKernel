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
pub(crate) mod render;
pub(crate) mod state;

pub use state::AnsiTerm;

use core::sync::atomic::{AtomicBool, Ordering};

// Tick-stamp storage uses the widest atomically-accessible integer on the
// target (same discipline as srv::timer): rv32 has no 64-bit AMOs, so
// AtomicU64 does not exist there. Widened to u64 for all arithmetic.
#[cfg(target_pointer_width = "32")]
use core::sync::atomic::AtomicU32 as AtomicTickStore;
#[cfg(target_pointer_width = "64")]
use core::sync::atomic::AtomicU64 as AtomicTickStore;

/// Global console state used by the sys_write path.
pub(crate) static mut G_ANSI: AnsiTerm = AnsiTerm::new();

// ── Frame lifecycle (TUI anti-flicker) ───────────────────────────────────
//
// Console output used to paint straight into the visible framebuffer, so a
// full-screen TUI redraw (`ESC[2J` + thousands of per-byte character paints,
// several write() chunks per frame) kept cleared/partial states on the
// screen for tens of milliseconds — the flicker reported for vim/otop
// (alternating frames: status line missing, cursor block at an intermediate
// position).
//
// With the fb back buffer active (fb::double_buffered), output paints
// off-screen and the presenter below publishes whole frames only:
//   * console_putc / console_cursor mark the frame dirty;
//   * present_tick runs once per 100 Hz timer tick per hart and presents
//     the back buffer when the frame is dirty AND the console has been
//     quiet for PRESENT_IDLE_TICKS (a redraw's chunks land microseconds
//     apart, so a quiet tick means the application finished its frame) —
//     or when the frame has stayed dirty for PRESENT_FORCE_TICKS so a
//     continuous output stream (cat, logs) still moves at ~7 fps;
//   * the present runs under the console write lock (try_lock: skip when
//     a writer on another hart is mid-frame) so a frame is NEVER copied
//     mid-write, and the software cursor is stamped onto the presented
//     frame only (it never bakes into the back buffer).
//
// Present latency: <= 20 ms after the last keystroke/redraw (2 idle ticks)
// — invisible to humans; worst case (continuous output) 150 ms.

/// Jiffies-of-idle required before a dirty frame is presented (2 ticks
/// = 20 ms at the 100 Hz CLINT tick): a full TUI redraw's write() chunks
/// are microseconds apart, so two silent ticks mean the frame is complete.
pub const PRESENT_IDLE_TICKS: u64 = 2;
/// Max ticks a frame may stay dirty before a forced present (15 ticks
/// = 150 ms), keeping continuous output streams visibly alive.
pub const PRESENT_FORCE_TICKS: u64 = 15;

static G_DIRTY: AtomicBool = AtomicBool::new(false);
/// Jiffies of the most recent console write (any byte).
static G_LAST_WRITE: AtomicTickStore = AtomicTickStore::new(0);
/// Jiffies when the current dirty streak began (for the force threshold).
static G_DIRTY_SINCE: AtomicTickStore = AtomicTickStore::new(0);

#[inline]
fn now_ticks() -> u64 {
    crate::srv::timer::jiffies()
}

#[inline]
fn load_tick(a: &AtomicTickStore) -> u64 {
    a.load(Ordering::Acquire) as u64
}

#[inline]
fn store_tick(a: &AtomicTickStore, v: u64) {
    // rv32: deliberate truncating store — jiffies wrap on rv32 is handled
    // by the wrapping idle/held arithmetic (same policy as srv::timer).
    #[cfg(target_pointer_width = "32")]
    a.store(v as u32, Ordering::Release);
    #[cfg(target_pointer_width = "64")]
    a.store(v, Ordering::Release);
}

/// Record that the console frame changed (new output or cursor state).
#[inline]
fn mark_dirty() {
    let now = now_ticks();
    store_tick(&G_LAST_WRITE, now);
    if !G_DIRTY.swap(true, Ordering::AcqRel) {
        store_tick(&G_DIRTY_SINCE, now);
    }
}

/// Pure present decision (unit-tested): present a dirty frame when the
/// console has gone quiet for PRESENT_IDLE_TICKS, or when it has been
/// continuously dirty for PRESENT_FORCE_TICKS (continuous-stream cap).
fn should_present(dirty: bool, idle: u64, held: u64) -> bool {
    dirty && (idle >= PRESENT_IDLE_TICKS || held >= PRESENT_FORCE_TICKS)
}

/// Write a byte through the ANSI interpreter.
pub fn console_putc(c: u8) {
    // SAFETY: G_ANSI is a kernel-lifetime static; SIE=0 prevents same-hart preemption (see crate::sync). Cross-hart concurrent console_putc IS possible (klog from trap contexts) and tolerated by convention: output may interleave, the static never reallocates, and no reference escapes this block.
    unsafe {
        let t = &raw mut G_ANSI;
        (*t).putc(c);
    }
    // The byte painted the (off-screen) paint surface: flag the frame.
    mark_dirty();
}

/// Refresh the cursor after a console operation.
///
/// Double-buffered mode: the cursor is stamped onto the frame by the
/// presenter (draw_cursor_front) at present time — legacy per-write cursor
/// painting is what drew mid-redraw cursor blocks at garbage positions
/// (the second artifact in the flicker report). Just request a present.
pub fn console_cursor() {
    if crate::drivers::fb::double_buffered() {
        mark_dirty();
    } else {
        // SAFETY: G_ANSI is a kernel-lifetime static; SIE=0 prevents same-hart preemption (see crate::sync); cross-hart interleaving is tolerated by convention (see console_putc); draw_cursor only repaints the console grid.
        unsafe {
            let t = &raw mut G_ANSI;
            (*t).draw_cursor();
        }
    }
}

/// Present the pending frame if it is due. Called once per timer tick
/// (100 Hz) from srv::timer::handle on every hart; cheap no-op unless the
/// frame is dirty and due. Never blocks and never allocates (safe in
/// interrupt context): the try_lock skips ticks where a writer holds the
/// console lock, and the back buffer is allocated eagerly at fb init.
pub fn present_tick() {
    if !crate::drivers::fb::double_buffered() {
        return;
    }
    let dirty = G_DIRTY.load(Ordering::Acquire);
    if !dirty {
        return;
    }
    let now = now_ticks();
    let idle = now.wrapping_sub(load_tick(&G_LAST_WRITE));
    let held = now.wrapping_sub(load_tick(&G_DIRTY_SINCE));
    if !should_present(dirty, idle, held) {
        return;
    }
    // Skip while a console writer (sys_write/echo on another hart, or a
    // presenter that won the race) holds the lock: the frame would be
    // half-written. The next tick retries.
    if !crate::srv::klog::UART_LOCK.try_lock() {
        return;
    }
    present_locked();
    crate::srv::klog::UART_LOCK.unlock();
}

/// Force an immediate present (debug/synchronous users). Blocks on the
/// console write lock — do NOT call from interrupt context.
pub fn present_now() {
    if !crate::drivers::fb::double_buffered() {
        return;
    }
    crate::srv::klog::UART_LOCK.lock();
    present_locked();
    crate::srv::klog::UART_LOCK.unlock();
}

/// Copy the back buffer to the front and stamp the cursor onto the
/// presented frame. Caller must hold the console write lock.
fn present_locked() {
    // SAFETY: G_ANSI is a kernel-lifetime static; the caller holds the
    // console write lock, excluding concurrent writers/presenters; the
    // borrow does not escape this block.
    unsafe {
        let t = &raw mut G_ANSI;
        crate::drivers::fb::present();
        (*t).draw_cursor_front();
    }
    G_DIRTY.store(false, Ordering::Release);
}

#[cfg(test)]
mod tests {
    use super::{AnsiTerm, PRESENT_FORCE_TICKS, PRESENT_IDLE_TICKS, should_present};

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
}

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
    use super::{G_ANSI, console_cursor, console_putc, present_locked};
    use crate::drivers::fb;

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
        // Invariant 2: output landed in the back buffer (ESC[2J filled it
        // with the black bg over the zero-filled initial state — the blank
        // fallback font paints full-cell backgrounds, so the content differs
        // from a zeroed surface wherever rows were written... for a fully
        // black bg the bytes stay zero, so assert via present below instead:
        // present must make front byte-equal to back, which only holds if
        // the frame actually went through the back buffer).
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

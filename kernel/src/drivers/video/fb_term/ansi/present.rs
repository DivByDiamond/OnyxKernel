//! Frame lifecycle / presenter for the ANSI console (TUI anti-flicker).
//!
//! Console output paints off-screen into the fb back buffer when
//! `fb::double_buffered()` is active; this module decides when to publish
//! whole frames, stamping the software cursor only onto the visible front
//! buffer. Extracted from `ansi/mod.rs` to keep each file <250 lines
//! (info.md rule #1).

use core::sync::atomic::{AtomicBool, Ordering};

#[cfg(target_pointer_width = "32")]
use core::sync::atomic::AtomicU32 as AtomicTickStore;
#[cfg(target_pointer_width = "64")]
use core::sync::atomic::AtomicU64 as AtomicTickStore;

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
#[cfg(target_pointer_width = "32")]
fn load_tick(a: &AtomicTickStore) -> u64 {
    u64::from(a.load(Ordering::Acquire))
}

#[inline]
#[cfg(target_pointer_width = "64")]
fn load_tick(a: &AtomicTickStore) -> u64 {
    a.load(Ordering::Acquire)
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
pub(crate) fn mark_dirty() {
    let now = now_ticks();
    store_tick(&G_LAST_WRITE, now);
    if !G_DIRTY.swap(true, Ordering::AcqRel) {
        store_tick(&G_DIRTY_SINCE, now);
    }
}

/// Pure present decision (unit-tested): present a dirty frame when the
/// console has gone quiet for PRESENT_IDLE_TICKS, or when it has been
/// continuously dirty for PRESENT_FORCE_TICKS (continuous-stream cap).
pub(crate) fn should_present(dirty: bool, idle: u64, held: u64) -> bool {
    dirty && (idle >= PRESENT_IDLE_TICKS || held >= PRESENT_FORCE_TICKS)
}

/// Write a byte through the ANSI interpreter.
pub fn console_putc(c: u8) {
    // SAFETY: G_ANSI is a kernel-lifetime static; SIE=0 prevents same-hart preemption (see crate::sync). Cross-hart concurrent console_putc IS possible (klog from trap contexts) and tolerated by convention: output may interleave, the static never reallocates, and no reference escapes this block.
    unsafe {
        let t = &raw mut super::G_ANSI;
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
            let t = &raw mut super::G_ANSI;
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
pub(crate) fn present_locked() {
    // SAFETY: G_ANSI is a kernel-lifetime static; the caller holds the
    // console write lock, excluding concurrent writers/presenters; the
    // borrow does not escape this block.
    unsafe {
        let t = &raw mut super::G_ANSI;
        crate::drivers::fb::present();
        (*t).draw_cursor_front();
    }
    G_DIRTY.store(false, Ordering::Release);
}

#[cfg(test)]
mod tests {
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
}

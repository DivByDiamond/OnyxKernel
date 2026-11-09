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
    if super::super::graphics::active() {
        return;
    }
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
    if super::super::graphics::active() {
        return;
    }
    if crate::drivers::fb::double_buffered() {
        mark_dirty();
    } else {
        crate::srv::klog::UART_LOCK.lock();
        if !super::super::graphics::active() {
            // SAFETY: the console lock excludes graphics claims and writers.
            unsafe {
                let t = &raw mut super::G_ANSI;
                (*t).draw_cursor();
            }
        }
        crate::srv::klog::UART_LOCK.unlock();
    }
}

/// Present the pending frame if it is due. Called once per timer tick
/// (100 Hz) from srv::timer::handle on every hart; cheap no-op unless the
/// frame is dirty and due. Never blocks and never allocates (safe in
/// interrupt context): the try_lock skips ticks where a writer holds the
/// console lock, and the back buffer is allocated eagerly at fb init.
pub fn present_tick() {
    if super::super::graphics::active() {
        return;
    }
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
    if super::super::graphics::active() {
        return;
    }
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
    if super::super::graphics::active() {
        return;
    }
    let Some(publication) = crate::drivers::fb::present_control::Publication::begin_console()
    else {
        return;
    };
    // SAFETY: G_ANSI is a kernel-lifetime static; the caller holds the
    // console write lock, excluding concurrent writers/presenters; the
    // borrow does not escape this block.
    unsafe {
        let t = &raw mut super::G_ANSI;
        if !super::scale::present(&*t) {
            crate::drivers::fb::present_region(
                (*t).cols * super::state::FONT_W,
                (*t).rows * super::state::FONT_H,
            );
            (*t).draw_cursor_front();
        }
    }
    if publication.commit() {
        G_DIRTY.store(false, Ordering::Release);
    } else {
        // Host rejected publication: the consumed dirty region must be retried.
        unsafe {
            let t = &raw const super::G_ANSI;
            super::scale::mark(super::viewport::Rect::new(
                0,
                0,
                (*t).cols * super::state::FONT_W,
                (*t).rows * super::state::FONT_H,
            ));
        }
    }
}

#[cfg(test)]
mod tests;

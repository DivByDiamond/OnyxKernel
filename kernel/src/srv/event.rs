//! Kernel event loop service (todo P3 #3).
//!
//! A tiny heartbeat driven by the timer tick: pumps unified input sources
//! (virtio-input -> dispatch -> mouse state) and runs registered soft
//! timers. Soft timers are millisecond-resolution callbacks scheduled via
//! uptime_us() deadlines — enough for TUI blinking/resizing helpers; the
//! scheduler itself does not depend on this module.
//!
//! Integration: `timer::handle()` calls `pump()` every tick, and the
//! poll() syscall (todo P1 #1) keeps sleeping user processes yielding, so
//! a TUI loop { poll(stdin, timeout) + mouse_read + redraw } stays live.

use crate::drivers::input;

/// Soft-timer slots. Four is enough for the current consumers (TUI cursor
/// blink, periodic redraw); bump when a real subscriber appears.
pub const MAX_TIMERS: usize = 4;

#[derive(Clone, Copy)]
struct SoftTimer {
    used: bool,
    interval_us: u64,
    /// Absolute uptime_us() deadline of the next fire.
    deadline_us: u64,
    cb: fn(),
}

static mut G_TIMERS: [SoftTimer; MAX_TIMERS] = [SoftTimer {
    used: false,
    interval_us: 0,
    deadline_us: 0,
    cb: noop,
}; MAX_TIMERS];

fn noop() {}

/// Register a periodic callback (interval in microseconds). Fires first
/// after one full interval. Returns false when the table is full.
pub fn add_timer(interval_us: u64, cb: fn()) -> bool {
    if interval_us == 0 {
        return false;
    }
    // SAFETY: G_TIMERS is kernel-owned static state; registration happens
    // from kernel context (init / syscall path, SIE clear), and pump()
    // only reads `used` — both under the crate::sync no-preemption model.
    unsafe {
        let timers = &raw mut G_TIMERS;
        for slot in (*timers).as_mut_slice() {
            if !slot.used {
                *slot = SoftTimer {
                    used: true,
                    interval_us,
                    deadline_us: crate::srv::timer::uptime_us() + interval_us,
                    cb,
                };
                return true;
            }
        }
    }
    false
}

/// Remove all timers registered by `cb` (identity compare).
pub fn remove_timer(cb: fn()) {
    // SAFETY: see add_timer (kernel-owned static, kernel context).
    unsafe {
        let timers = &raw mut G_TIMERS;
        for slot in (*timers).as_mut_slice() {
            if slot.used && (slot.cb as usize) == (cb as usize) {
                slot.used = false;
            }
        }
    }
}

/// One heartbeat: pump input devices and fire due soft timers.
/// Called from the timer interrupt path (SIE clear, no preemption).
pub fn pump() {
    // Input first so timer callbacks observe fresh pointer/keys.
    input::poll_all();
    let now = crate::srv::timer::uptime_us();
    // SAFETY: G_TIMERS is kernel-owned static state; the IRQ path runs
    // with SIE clear (no same-hart re-entry) per the crate::sync model.
    unsafe {
        let timers = &raw mut G_TIMERS;
        for slot in (*timers).as_mut_slice() {
            if slot.used && now >= slot.deadline_us {
                slot.deadline_us = now + slot.interval_us;
                (slot.cb)();
            }
        }
    }
}

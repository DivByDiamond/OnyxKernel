//! Wall-clock base: RTC epoch + monotonic uptime.
//!
//! The RTC (see `drivers::platform::rtc`) reports nanoseconds since the
//! Unix epoch, but it is only readable at boot probe time. We read it once,
//! store the offset `rtc_us - uptime_us()`, and derive every wall-clock
//! query from `uptime_us() + offset` — so the clock never jumps when the
//! RTC is sampled again and stays monotonic as long as uptime is.
//!
//! Storage is two 32-bit atomics (seconds + sub-second remainder) rather
//! than one 64-bit one: rv32 has no 64-bit AMOs, and the offset is written
//! once at boot but read concurrently from every hart. Offset seconds are
//! signed (`i32`), so an RTC behind uptime (or a dead RTC reading a small
//! value) still produces a sane, possibly negative, offset.
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use super::timer::uptime_us;

/// Offset seconds, two's-complement `i32` bit pattern.
static G_OFFSET_S: AtomicU32 = AtomicU32::new(0);
/// Sub-second part of the offset, always in `0..1_000_000`.
static G_OFFSET_US: AtomicU32 = AtomicU32::new(0);
/// Set last with Release once both halves are stored; readers Acquire it
/// first so they never observe a half-updated offset.
static G_HAVE_RTC: AtomicBool = AtomicBool::new(false);

/// Split a microsecond offset into storable `(seconds, subsec_us)` parts.
/// Returns `None` when the seconds part does not fit `i32` (an implausible
/// RTC reading, e.g. beyond year 2038 in either direction) — the caller
/// then keeps the uptime-only fallback instead of storing garbage.
#[must_use]
const fn split_offset(off_us: i64) -> Option<(i32, u32)> {
    let secs = off_us.div_euclid(1_000_000);
    let sub = off_us.rem_euclid(1_000_000);
    if secs > i32::MAX as i64 || secs < i32::MIN as i64 {
        return None;
    }
    Some((secs as i32, sub as u32))
}

/// Establish the wall-clock base from an RTC reading in nanoseconds since
/// the Unix epoch. Call once at boot probe, after `timer::init()` and the
/// RTC probe, before secondary harts start querying the clock.
///
/// A zero `rtc_ns` (no RTC probed) and an implausible offset both leave the
/// previous state untouched, so `realtime_us()` degrades to uptime.
pub fn set_rtc_base(rtc_ns: u64) {
    if rtc_ns == 0 {
        return;
    }
    let Some((secs, sub)) = split_offset((rtc_ns / 1000) as i64 - uptime_us() as i64) else {
        return;
    };
    // Write halves first, publish with Release: a reader that sees
    // G_HAVE_RTC == true is guaranteed both halves are final (the base is
    // set exactly once, at boot).
    G_OFFSET_S.store(secs as u32, Ordering::Release);
    G_OFFSET_US.store(sub, Ordering::Release);
    G_HAVE_RTC.store(true, Ordering::Release);
}

/// True once an RTC base has been accepted.
#[must_use]
pub fn have_rtc() -> bool {
    G_HAVE_RTC.load(Ordering::Acquire)
}

/// Epoch seconds at boot (the stored offset is computed against uptime at
/// probe time, i.e. a few milliseconds after boot). 0 when no RTC base was
/// established — same fallback as [`realtime_us`].
#[must_use]
pub fn boot_epoch_secs() -> u64 {
    if !have_rtc() {
        return 0;
    }
    (G_OFFSET_S.load(Ordering::Acquire) as i32).max(0) as u64
}

/// Wall-clock microseconds since the Unix epoch: `uptime_us() + offset`,
/// or plain uptime when no RTC base was ever established (boot without an
/// RTC, or an implausible RTC reading).
#[must_use]
pub fn realtime_us() -> u64 {
    if !have_rtc() {
        return uptime_us();
    }
    let secs = G_OFFSET_S.load(Ordering::Acquire) as i32 as i64;
    let sub = G_OFFSET_US.load(Ordering::Acquire) as i64;
    (uptime_us() as i64).wrapping_add(secs * 1_000_000 + sub) as u64
}

#[cfg(test)]
mod tests;

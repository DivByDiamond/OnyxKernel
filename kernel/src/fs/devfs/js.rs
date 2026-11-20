//! /dev/jsN: joydev-compatible joystick event nodes.
//!
//! Each attached stick slot owns a static ring of 8-byte
//! joydev-compatible records (layout in [`rec`]): the 100 Hz input
//! drain pushes EVERY polled HID event into the attached session's
//! ring before any claim gating (event::feed_stick), so the ring
//! always fills regardless of cursor state; overflow drops the OLDEST
//! record — bounded history beats invented events.
//!
//! `js_read` delivers whole records only; an empty ring is
//! `Errno::Again`, never a fake EOF (`Ok(0)`). The open hook (the
//! vfs/fd/file.rs DEVFS branch, the PTMX pattern) claims the stick
//! and synthesizes the `JS_EVENT_INIT` burst from the session
//! baseline — axes first, then buttons; closing ANY js fd releases
//! the claim. The kernel keeps no per-fd refcount: a single claimed
//! flag is the contract (single-reader expectation, like the PTY pair
//! lifetime) — a second reader shares the claim, and whichever fd
//! closes first unclaims for both.
//!
//! Lock order: the push side runs under `DRAIN_LOCK` → `JS_LOCK`,
//! never reversed; `js_read` takes `JS_LOCK` alone; `js_claim` takes
//! `USB_HID_LOCK` (inside `hid::baseline`, FIRST — the claim latches
//! only on a live baseline), then `STICK_CLAIM_LOCK`, then `JS_LOCK`
//! — strictly one at a time, never nested.

// `&raw mut` on a `static mut` is deliberate: clippy's suggested bare
// `JS_RINGS` would create a reference to it and trip
// `static_mut_refs` (the same allowance the HID session carries).
#![allow(clippy::deref_addrof)]

mod rec;

use rec::{INIT_BURST_CAP, JsRec};
pub use rec::{JS_EVENT_AXIS, JS_EVENT_BUTTON, JS_EVENT_INIT, JS_REC_BYTES};

use core::ptr;
use onyx_core::errno::{Errno, KResult};

use crate::drivers::bus::usb::dev::MAX_SLOTS;
use crate::drivers::bus::usb::hid::{HidEvent, baseline, stick_slots};
use crate::drivers::input::event::stick_cursor;

#[cfg(test)]
mod record_tests;
#[cfg(test)]
mod tests;

/// Base inode for /dev/jsN: `js_ino(i) = DEVFS_JS_BASE_INO + i`,
/// `i < MAX_SLOTS`. Kept at the brief's 80 — clear of every inode
/// range that can exist at plausible device counts: fb nodes grow
/// from `DEVFS_EXTRA_FB_BASE_INO` (64) with the display count (a
/// collision needs 17 displays), block nodes from 16 (needs 64
/// virtio-blk devices), PTY masters hold 32..35 and slaves 40..43
/// (PTY_MAX = 4), ptmx is 48, hvc0 is 49.
pub const DEVFS_JS_BASE_INO: u32 = 80;
/// Ring capacity per stick slot (records; × 8 bytes = 512).
pub const JS_RING_RECS: usize = 64;

#[inline]
pub const fn js_ino(idx: u32) -> u32 {
    DEVFS_JS_BASE_INO + idx
}

#[inline]
pub const fn is_js_ino(ino: u32) -> Option<usize> {
    let last = DEVFS_JS_BASE_INO + MAX_SLOTS as u32;
    if ino >= DEVFS_JS_BASE_INO && ino < last {
        Some((ino - DEVFS_JS_BASE_INO) as usize)
    } else {
        None
    }
}

/// Per-slot record ring: FIFO of whole records; a full ring drops the
/// OLDEST record on push (documented policy: bounded history beats
/// invented events).
pub(crate) struct Ring {
    recs: [JsRec; JS_RING_RECS],
    head: usize,
    len: usize,
}

impl Ring {
    pub(crate) const fn new() -> Self {
        Ring {
            recs: [const { JsRec::zero() }; JS_RING_RECS],
            head: 0,
            len: 0,
        }
    }

    fn push(&mut self, rec: JsRec) {
        if self.len == JS_RING_RECS {
            self.head = (self.head + 1) % JS_RING_RECS;
            self.len -= 1;
        }
        self.recs[(self.head + self.len) % JS_RING_RECS] = rec;
        self.len += 1;
    }

    fn pop(&mut self) -> Option<JsRec> {
        if self.len == 0 {
            return None;
        }
        let rec = self.recs[self.head];
        self.head = (self.head + 1) % JS_RING_RECS;
        self.len -= 1;
        Some(rec)
    }
}

static JS_LOCK: crate::sync::SpinLock = crate::sync::SpinLock::new();
static mut JS_RINGS: [Ring; MAX_SLOTS] = [const { Ring::new() }; MAX_SLOTS];

/// The same ms-uptime source the srv/wallclock area uses, truncated
/// to joydev's u32 stamp (wraps at ~49.7 days, like the original).
fn now_ms() -> u32 {
    (crate::srv::timer::uptime_us() / 1_000) as u32
}

/// Push one HID event into slot `slot_idx`'s ring as a record — the
/// input-drain hook (event::feed_stick): every polled event lands
/// here BEFORE the raw dispatch and the claim-gated mapping, so the
/// ring fills regardless of claim state. Out-of-range slots are
/// ignored — the drain carries the attached session's slot on each
/// poll result, never invents one.
pub fn push_event(slot_idx: usize, ev: HidEvent, init: bool) {
    if slot_idx >= MAX_SLOTS {
        return;
    }
    let rec = JsRec::from_event(ev, init, now_ms());
    JS_LOCK.lock();
    // SAFETY: JS_LOCK serializes ring access; slot_idx < MAX_SLOTS
    // was checked above.
    unsafe {
        (*(&raw mut JS_RINGS))[slot_idx].push(rec);
    }
    JS_LOCK.unlock();
}

/// Read up to `len / JS_REC_BYTES` whole records from slot `idx`'s
/// ring. An empty ring — or a buffer too small for one record — is
/// `Err(Errno::Again)`, never `Ok(0)` (which would read as EOF); the
/// `len % 8` tail stays in the ring (partial records are not
/// delivered).
///
/// # Safety
///
/// Caller contract: `buf` must be writable for `len` bytes — for user
/// callers the syscall layer validated and translated the range (the
/// same contract the other devfs read branches document); `idx` is
/// re-checked against MAX_SLOTS here.
pub unsafe fn js_read(idx: usize, buf: *mut u8, len: u32) -> KResult<u32> {
    if idx >= MAX_SLOTS {
        return Err(Errno::NoEnt);
    }
    JS_LOCK.lock();
    // SAFETY: JS_LOCK serializes ring access; idx was bounds-checked
    // above, and buf covers len bytes per the caller contract — the
    // copy inside never exceeds it.
    let n = unsafe {
        let rings = &mut *(&raw mut JS_RINGS);
        let ring = &mut rings[idx];
        let fit = (len as usize / JS_REC_BYTES).min(ring.len);
        if fit == 0 {
            Err(Errno::Again)
        } else {
            for i in 0..fit {
                let Some(rec) = ring.pop() else {
                    break; // fit <= ring.len: unreachable, defensive only
                };
                // SAFETY: i < fit <= len/8, so the 8-byte store stays
                // within the caller's len-byte buffer.
                ptr::copy_nonoverlapping(
                    rec.bytes().as_ptr(),
                    buf.add(i * JS_REC_BYTES),
                    JS_REC_BYTES,
                );
            }
            Ok((fit * JS_REC_BYTES) as u32)
        }
    };
    JS_LOCK.unlock();
    n
}

/// Open hook (the vfs/fd/file.rs DEVFS branch, PTMX pattern): claim
/// the stick for userspace and synthesize the open-time init burst
/// into the slot's ring from the session baseline. The baseline
/// resolves FIRST: a slot without an attached session (lookup gates
/// the node on attachment, so this is at worst a same-slot race
/// window) must not latch the claim — a claimed stick with no reader
/// would silence the cursor mapper until some fd closes — and it
/// pushes nothing.
pub fn js_claim(idx: usize) {
    let Some(base) = baseline(idx) else {
        return; // idx is out of range or has no session: nothing to claim
    };
    stick_cursor::set_claimed(true);
    let now = now_ms();
    let mut recs = [const { JsRec::zero() }; INIT_BURST_CAP];
    let n = rec::init_recs(&base, now, &mut recs);
    JS_LOCK.lock();
    // SAFETY: JS_LOCK serializes ring access; baseline() returned
    // Some, so idx < MAX_SLOTS.
    unsafe {
        let rings = &mut *(&raw mut JS_RINGS);
        for rec in &recs[..n] {
            rings[idx].push(*rec);
        }
    }
    JS_LOCK.unlock();
}

/// Close hook: closing ANY /dev/jsN fd releases the single stick
/// claim — no per-fd refcount exists (single-reader contract, see the
/// module docs), like the PTY pair's close lifetime.
pub fn js_release() {
    stick_cursor::set_claimed(false);
}

/// Does registry slot `idx` currently hold an attached stick session?
/// The /dev/jsN namespace gate (lookup).
pub(crate) fn listed(idx: usize) -> bool {
    idx < MAX_SLOTS && stick_slots()[idx]
}

/// Attached stick slots in slot order — the readdir list backing.
pub(crate) fn attached() -> [bool; MAX_SLOTS] {
    stick_slots()
}

//! Stick → cursor mapper with claim semantics.
//!
//! While no userspace consumer holds a given stick (the /dev/jsN
//! open/close path of the devfs ring calls [`claim`]/[`release`], a
//! per-slot open count), the 100 Hz input drain feeds that stick's HID
//! events through [`map`] and dispatches the result as ordinary mouse
//! events — an unclaimed stick drives the kernel cursor. A claimed
//! stick stops mapping (other sticks are unaffected):
//! only the raw `Event::Stick*` stream flows on to the global handler
//! (and, from task 9 on, the per-device ring).
//!
//! Sign convention: `mouse::apply` integrates +dy downward (screen Y
//! grows down), so a NEGATIVE Y axis value — the stick pushed up —
//! maps to a negative dy and moves the cursor UP. X is symmetric:
//! negative values move left.
//!
//! Lock order: the input drain holds `DRAIN_LOCK` and then takes
//! `STICK_CLAIM_LOCK` here; the devfs claim paths take
//! `STICK_CLAIM_LOCK` alone — never the reverse.

use super::{Event, MouseButton};
use crate::drivers::bus::usb::dev::MAX_SLOTS;
use crate::drivers::bus::usb::hid::HidEvent;

/// Below this |value| an axis never emits a cursor event.
pub const DEAD_ZONE: i16 = 3000;
/// Arithmetic gain: `dx = value >> 12` → max ±7 px/tick.
pub const GAIN_SHIFT: u32 = 12;
/// Axis id (first-appearance descriptor order) treated as cursor X.
pub const CURSOR_AXIS_X: u8 = 0;
/// Axis id treated as cursor Y.
pub const CURSOR_AXIS_Y: u8 = 1;
/// Button 0 = left click.
pub const CURSOR_BUTTON: u8 = 0;

static STICK_CLAIM_LOCK: crate::sync::SpinLock = crate::sync::SpinLock::new();
/// Per-slot count of open /dev/jsN fds that latched a claim. Per slot
/// (not global): closing js0 must not hand js1's stick back to the
/// cursor mapper.
static mut G_CLAIMS: [u8; MAX_SLOTS] = [0; MAX_SLOTS];
/// Per-slot input owner: the pid that last opened (or was resumed
/// with) the node — the foreground consumer the desktop switches to.
/// 0 = no known owner. Informational for the switcher: a SIGSTOP of
/// the owner never drops the claim (the app still has the fd open),
/// only closing the last fd does.
static mut G_OWNERS: [u32; MAX_SLOTS] = [0; MAX_SLOTS];

/// Make `pid` the owner of slot `slot` (open, or SIGCONT resume).
/// Out-of-range slots are ignored.
pub fn set_owner(slot: usize, pid: u32) {
    if slot >= MAX_SLOTS {
        return;
    }
    STICK_CLAIM_LOCK.lock();
    // SAFETY: STICK_CLAIM_LOCK serializes the owner table; slot was
    // bounds-checked; no reference to the static is taken.
    unsafe {
        G_OWNERS[slot] = pid;
    }
    STICK_CLAIM_LOCK.unlock();
}

/// Owner pid of `slot` (0 = none or out of range).
pub fn owner(slot: usize) -> u32 {
    if slot >= MAX_SLOTS {
        return 0;
    }
    STICK_CLAIM_LOCK.lock();
    // SAFETY: as in `set_owner`; plain copy-out.
    let pid = unsafe { G_OWNERS[slot] };
    STICK_CLAIM_LOCK.unlock();
    pid
}

/// Take one claim on registry slot `slot` (a /dev/jsN open that
/// latched on a live baseline). Out-of-range slots are ignored; the
/// count saturates. Kernel context with interrupts disabled, the same
/// discipline `MOUSE_LOCK` documents.
pub fn claim(slot: usize) {
    if slot >= MAX_SLOTS {
        return;
    }
    STICK_CLAIM_LOCK.lock();
    // SAFETY: STICK_CLAIM_LOCK serializes the counters; slot < MAX_SLOTS
    // was checked above; direct place access, no reference to the
    // static is taken (the fb/mod.rs pattern).
    unsafe {
        G_CLAIMS[slot] = G_CLAIMS[slot].saturating_add(1);
    }
    STICK_CLAIM_LOCK.unlock();
}

/// Drop one claim on `slot` (a /dev/jsN close). The count saturates at
/// zero: a close whose open never latched (session-less slot) or a
/// double close of a dup'd fd cannot underflow. Out-of-range ignored.
pub fn release(slot: usize) {
    if slot >= MAX_SLOTS {
        return;
    }
    STICK_CLAIM_LOCK.lock();
    // SAFETY: as in `claim`.
    unsafe {
        G_CLAIMS[slot] = G_CLAIMS[slot].saturating_sub(1);
        // The last close leaves nobody to own the stick.
        if G_CLAIMS[slot] == 0 {
            G_OWNERS[slot] = 0;
        }
    }
    STICK_CLAIM_LOCK.unlock();
}

/// Is a userspace consumer currently claiming the stick of `slot`?
/// Out-of-range slots are never claimed.
pub fn claimed(slot: usize) -> bool {
    if slot >= MAX_SLOTS {
        return false;
    }
    STICK_CLAIM_LOCK.lock();
    // SAFETY: the lock excludes every writer of the counters; plain
    // copy-out, no reference taken; slot < MAX_SLOTS.
    let value = unsafe { G_CLAIMS[slot] } > 0;
    STICK_CLAIM_LOCK.unlock();
    value
}

/// Pure stick → mouse mapping. Axes 0/1 outside the dead zone become
/// relative motion (`>> GAIN_SHIFT`, arithmetic shift — negative
/// values move left/up); button 0 becomes the left button; every
/// other axis or button maps to nothing. Dead-zone edge note:
/// DEAD_ZONE < 2^GAIN_SHIFT, so values in [3000, 4095] pass the gate
/// but move 0 px.
pub fn map(event: HidEvent) -> Option<Event> {
    match event {
        HidEvent::Axis { axis, value } => {
            if value.unsigned_abs() < DEAD_ZONE.unsigned_abs() {
                return None;
            }
            match axis {
                CURSOR_AXIS_X => Some(Event::MouseRel {
                    dx: i32::from(value >> GAIN_SHIFT),
                    dy: 0,
                }),
                CURSOR_AXIS_Y => Some(Event::MouseRel {
                    dx: 0,
                    dy: i32::from(value >> GAIN_SHIFT),
                }),
                _ => None,
            }
        }
        HidEvent::Button { index, down } => {
            if index == CURSOR_BUTTON {
                Some(Event::MouseButton {
                    btn: MouseButton::Left,
                    down,
                })
            } else {
                None
            }
        }
    }
}

#[cfg(test)]
mod claim_tests;
#[cfg(test)]
mod tests;

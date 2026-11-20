//! Stick → cursor mapper with claim semantics.
//!
//! While no userspace consumer holds the joystick (the /dev/jsN
//! open/close path of the devfs ring, task 9, calls [`set_claimed`]),
//! the 100 Hz input drain feeds every HID event through [`map`] and
//! dispatches the result as ordinary mouse events — an unclaimed
//! stick drives the kernel cursor. A claimed stick stops mapping:
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
static mut G_CLAIMED: bool = false;

/// Claim (true) or release (false) the stick as a userspace joystick.
/// Called from the /dev/jsN open/close path (task 9); kernel context
/// with interrupts disabled, the same discipline `MOUSE_LOCK` documents.
pub fn set_claimed(claimed: bool) {
    STICK_CLAIM_LOCK.lock();
    // SAFETY: STICK_CLAIM_LOCK serializes the flag; callers run with
    // interrupts disabled as crate::sync requires. Direct place access,
    // no reference to the static is taken (the fb/mod.rs pattern).
    unsafe {
        G_CLAIMED = claimed;
    }
    STICK_CLAIM_LOCK.unlock();
}

/// Is a userspace consumer currently claiming the stick?
pub fn claimed() -> bool {
    STICK_CLAIM_LOCK.lock();
    // SAFETY: the lock excludes every devfs writer of the flag; plain
    // copy-out, no reference taken.
    let value = unsafe { G_CLAIMED };
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
mod tests;

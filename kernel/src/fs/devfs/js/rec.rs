//! The joydev record: wire layout, HID-event encoding, and the pure
//! open-time init-burst core. No statics, no locks — the ring lives
//! in the parent module, which keeps this pure core host-testable.

use crate::drivers::bus::usb::hid::{Baseline, HidEvent};

/// joydev button event type (linux/joystick.h).
pub const JS_EVENT_BUTTON: u8 = 0x01;
/// joydev axis event type.
pub const JS_EVENT_AXIS: u8 = 0x02;
/// joydev init flag, OR'ed into the type for the open-time burst.
pub const JS_EVENT_INIT: u8 = 0x80;
/// joydev record size: u32 time_ms + i16 value + u8 type + u8 number.
pub const JS_REC_BYTES: usize = 8;
/// Init-burst buffer: the baseline caps are 16 axes + 32 buttons.
pub(crate) const INIT_BURST_CAP: usize = 48;

/// One joydev-compatible record (the `struct js_event` wire shape):
/// `time_ms` u32 LE, `value` i16 LE, `type` u8, `number` u8.
#[derive(Clone, Copy, PartialEq)]
pub(crate) struct JsRec {
    pub(crate) time_ms: u32,
    pub(crate) value: i16,
    pub(crate) ty: u8,
    pub(crate) number: u8,
}

impl JsRec {
    pub(crate) const fn zero() -> Self {
        JsRec {
            time_ms: 0,
            value: 0,
            ty: 0,
            number: 0,
        }
    }

    /// The 8 wire bytes: time LE, value LE, type, number.
    pub(crate) fn bytes(&self) -> [u8; JS_REC_BYTES] {
        let t = self.time_ms.to_le_bytes();
        let v = self.value.to_le_bytes();
        [t[0], t[1], t[2], t[3], v[0], v[1], self.ty, self.number]
    }

    /// Raw HID event → record. An axis carries its raw i16 and id; a
    /// button its 0/1 edge value and index; `init` ORs
    /// `JS_EVENT_INIT` into the type.
    pub(crate) fn from_event(ev: HidEvent, init: bool, time_ms: u32) -> Self {
        let init = if init { JS_EVENT_INIT } else { 0 };
        match ev {
            HidEvent::Axis { axis, value } => JsRec {
                time_ms,
                value,
                ty: JS_EVENT_AXIS | init,
                number: axis,
            },
            HidEvent::Button { index, down } => JsRec {
                time_ms,
                value: i16::from(down),
                ty: JS_EVENT_BUTTON | init,
                number: index,
            },
        }
    }
}

/// Pure init-burst core: `base` → open-time records, axes first then
/// buttons (the ruling order), every type OR'ed with
/// `JS_EVENT_INIT`; returns the record count. The counts are clamped
/// to the baseline widths (the snapshot already clamps; double guard
/// for direct test callers).
pub(crate) fn init_recs(base: &Baseline, now: u32, out: &mut [JsRec]) -> usize {
    let mut n = 0usize;
    let axes = base.axes.iter().take(base.axis_n.min(base.axes.len()));
    for (a, &value) in axes.enumerate() {
        let Some(slot) = out.get_mut(n) else {
            return n;
        };
        *slot = JsRec {
            time_ms: now,
            value,
            ty: JS_EVENT_AXIS | JS_EVENT_INIT,
            number: a as u8,
        };
        n += 1;
    }
    for b in 0..base.button_n.min(u32::BITS as usize) {
        let Some(slot) = out.get_mut(n) else {
            break;
        };
        *slot = JsRec {
            time_ms: now,
            value: i16::from((base.buttons >> b) & 1 != 0),
            ty: JS_EVENT_BUTTON | JS_EVENT_INIT,
            number: b as u8,
        };
        n += 1;
    }
    n
}

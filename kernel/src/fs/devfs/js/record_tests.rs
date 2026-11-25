//! Host tests for the /dev/jsN joydev record surface: the byte-exact
//! wire layout (LE, joydev field order, the init bit OR'ed into the
//! type), the pure init-burst synthesis, and the open/close claim
//! hooks. Ring-state tests are self-contained (own slot + helpers) so
//! parallel test harts cannot interfere — the fetch_tests.rs pattern.

use super::rec::{JS_EVENT_AXIS, JS_EVENT_BUTTON, JS_EVENT_INIT, JsRec, init_recs};
use super::{Ring, js_claim, js_release, push_event};
use crate::drivers::bus::usb::hid::{Baseline, HidEvent};
use onyx_core::errno::KResult;

/// Reset slot `idx`'s ring under the ring lock (test isolation).
fn reset(idx: usize) {
    super::JS_LOCK.lock();
    // SAFETY: JS_LOCK is held; idx is a test slot (< MAX_SLOTS).
    unsafe {
        (*(&raw mut super::JS_RINGS))[idx] = Ring::new();
    }
    super::JS_LOCK.unlock();
}

/// One `js_read` call into a fresh 512-byte kernel buffer.
fn read_bytes(idx: usize, len: u32) -> (KResult<u32>, [u8; 512]) {
    let mut buf = [0u8; 512];
    // SAFETY: buf is a kernel stack buffer of 512 bytes, len <= 512.
    let r = unsafe { super::js_read(idx, buf.as_mut_ptr(), len) };
    (r, buf)
}

/// Slot 1: byte-exact wire layout of axis/button records and the
/// init bit OR'ed into the type byte.
#[test]
fn record_layout_is_joydev_little_endian() {
    reset(1);
    // Axis 2 at -4660 (0xEDCC): value bytes CC ED, type 0x02, number 2.
    push_event(
        1,
        HidEvent::Axis {
            axis: 2,
            value: -4660,
        },
        false,
    );
    let (r, b) = read_bytes(1, 8);
    assert_eq!(r, Ok(8));
    // Host tests never tick uptime: time_ms is 0 (u32 LE).
    assert_eq!(&b[..4], &[0, 0, 0, 0]);
    assert_eq!(&b[4..6], &[0xCC, 0xED]);
    assert_eq!(b[6], JS_EVENT_AXIS);
    assert_eq!(b[7], 2);

    // Button 1 press: value 1, type 0x01, number 1.
    push_event(
        1,
        HidEvent::Button {
            index: 1,
            down: true,
        },
        false,
    );
    let (r, b) = read_bytes(1, 8);
    assert_eq!(r, Ok(8));
    assert_eq!(&b[4..6], &[1, 0]);
    assert_eq!(b[6], JS_EVENT_BUTTON);
    assert_eq!(b[7], 1);

    // Button release carries value 0.
    push_event(
        1,
        HidEvent::Button {
            index: 1,
            down: false,
        },
        false,
    );
    let (r, b) = read_bytes(1, 8);
    assert_eq!(r, Ok(8));
    assert_eq!(&b[4..6], &[0, 0]);
    assert_eq!(b[6], JS_EVENT_BUTTON);

    // The session-init flag ORs into the type (0x82 / 0x81).
    push_event(1, HidEvent::Axis { axis: 0, value: 7 }, true);
    let (r, b) = read_bytes(1, 8);
    assert_eq!(r, Ok(8));
    assert_eq!(b[6], JS_EVENT_AXIS | JS_EVENT_INIT);
    push_event(
        1,
        HidEvent::Button {
            index: 0,
            down: true,
        },
        true,
    );
    let (r, b) = read_bytes(1, 8);
    assert_eq!(r, Ok(8));
    assert_eq!(b[6], JS_EVENT_BUTTON | JS_EVENT_INIT);
    assert_eq!(&b[4..6], &[1, 0]);
}

#[test]
fn init_burst_synthesizes_axes_then_buttons_with_init_bit() {
    let base = Baseline {
        axes: [5, -7, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        axis_n: 2,
        buttons: 0b101,
        button_n: 3,
    };
    let mut recs = [JsRec::zero(); 8];
    let n = init_recs(&base, 0x4433_2211, &mut recs);
    assert_eq!(n, 5);
    // Axes first, init-flagged, raw baseline values.
    assert_eq!(
        recs[0].bytes(),
        [
            0x11,
            0x22,
            0x33,
            0x44,
            0x05,
            0x00,
            JS_EVENT_AXIS | JS_EVENT_INIT,
            0
        ]
    );
    assert_eq!(
        recs[1].bytes(),
        [
            0x11,
            0x22,
            0x33,
            0x44,
            0xF9,
            0xFF,
            JS_EVENT_AXIS | JS_EVENT_INIT,
            1
        ]
    );
    // Buttons after: 0/1 from the bitmap, one record per button.
    assert_eq!(
        recs[2].bytes(),
        [
            0x11,
            0x22,
            0x33,
            0x44,
            0x01,
            0x00,
            JS_EVENT_BUTTON | JS_EVENT_INIT,
            0
        ]
    );
    assert_eq!(
        recs[3].bytes(),
        [
            0x11,
            0x22,
            0x33,
            0x44,
            0x00,
            0x00,
            JS_EVENT_BUTTON | JS_EVENT_INIT,
            1
        ]
    );
    assert_eq!(
        recs[4].bytes(),
        [
            0x11,
            0x22,
            0x33,
            0x44,
            0x01,
            0x00,
            JS_EVENT_BUTTON | JS_EVENT_INIT,
            2
        ]
    );
}

#[test]
fn claim_hooks_claim_and_release_the_stick() {
    // Shares the global claim flag with the stick_cursor test: both
    // serialize on CLAIM_TEST_GUARD (the net::lock::test_serial
    // pattern) so parallel harts cannot interleave.
    let _guard = crate::drivers::input::event::CLAIM_TEST_GUARD
        .lock()
        .unwrap();
    // Host tests attach no HID session, so js_claim resolves no
    // baseline: the claim must NOT latch on a session-less slot.
    js_claim(0);
    assert!(!crate::drivers::input::event::stick_cursor::claimed(0));
    // The release hook still clears a claim latched by the live path
    // (open with a session attached).
    crate::drivers::input::event::stick_cursor::claim(0);
    js_release(0);
    assert!(!crate::drivers::input::event::stick_cursor::claimed(0));
}

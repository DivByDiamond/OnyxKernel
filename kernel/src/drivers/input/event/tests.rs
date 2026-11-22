use super::*;
use core::sync::atomic::{AtomicU8, Ordering};

#[test]
fn a_reentrant_drain_cannot_dispatch_newer_events_before_the_current_one() {
    let lock = crate::sync::SpinLock::new();
    let mut events = [
        virtio_input::EventType::Key(KeyCode::LeftShift, true),
        virtio_input::EventType::Key(KeyCode::Letter(b'a'), true),
    ]
    .into_iter();
    let mut received = std::vec::Vec::new();
    with_drain_lock(&lock, || {
        drain_with(
            || events.next(),
            |raw| {
                // A reentrant drain must not run at all while the outer
                // pass holds the lock — its events stay queued.
                with_drain_lock(&lock, || panic!("reentrant drain must not run"));
                if let Some(Event::Key { code, .. }) = virtio_input::event_type(raw) {
                    received.push(code);
                }
            },
        );
    });
    assert_eq!(received, [KeyCode::LeftShift, KeyCode::Letter(b'a')]);
    assert!(lock.try_lock());
    lock.unlock();
}

#[test]
fn either_shift_and_ctrl_remain_active_until_both_sides_are_released() {
    let flags = AtomicU8::new(0);
    key::update_modifiers(&flags, KeyCode::LeftShift, true);
    key::update_modifiers(&flags, KeyCode::RightShift, true);
    key::update_modifiers(&flags, KeyCode::LeftShift, false);
    assert_ne!(flags.load(Ordering::Acquire) & 3, 0);
    key::update_modifiers(&flags, KeyCode::RightShift, false);
    assert_eq!(flags.load(Ordering::Acquire) & 3, 0);
    key::update_modifiers(&flags, KeyCode::LeftCtrl, true);
    key::update_modifiers(&flags, KeyCode::RightCtrl, true);
    key::update_modifiers(&flags, KeyCode::RightCtrl, false);
    assert_ne!(flags.load(Ordering::Acquire) & 12, 0);
    key::update_modifiers(&flags, KeyCode::LeftCtrl, false);
    assert_eq!(flags.load(Ordering::Acquire), 0);
}

/// The dual-controller SG2000 shape: a registry setup where the
/// HID-class slot differs from the SESSION slot must route /dev/jsN
/// ring pushes to the session slot. The decoy (registry slot 1, an
/// active HID-class device with no attached session — the EHCI
/// keyboard) is what a class-based lookup would wrongly pick; the
/// drain-carried session slot (0) is where the record must land.
/// Registry slots 0 and 2 belong to the other registry tests, so
/// parallel test harts cannot interfere, and ring 0 is private to
/// this test (the stick_cursor feed_stick caller passes an
/// out-of-range slot, so it never writes a ring).
#[test]
fn stick_events_route_to_the_session_slot_not_the_hid_class_slot() {
    use crate::drivers::usb::dev::{self, desc::CLASS_HID};
    use crate::drivers::usb::hid::HidEvent;
    use crate::fs::devfs::{JS_EVENT_AXIS, js_read};

    let decoy = dev::UsbSlot {
        active: true,
        owner: dev::Owner::Ehci,
        addr: 1,
        vendor: 0x0627,
        product: 0x0001,
        class: CLASS_HID,
        iface: 0,
        ep_in: 0x81,
        max_packet: 8,
        interval: 10,
        hc_slot: 0,
    };
    assert_eq!(dev::set_slot(1, decoy), Ok(()));

    // Drain ring 0 (defensive: leave nothing from an earlier pass),
    // then feed one axis event on the SESSION slot 0 — a no-op output
    // keeps the global handler/mouse state out of this test.
    let mut buf = [0u8; 8];
    // SAFETY: buf is a kernel stack buffer of 8 bytes; slot 0 is in
    // range, so js_read stays inside its ring.
    while unsafe { js_read(0, buf.as_mut_ptr(), 8) }.is_ok() {}
    feed_stick(
        HidEvent::Axis {
            axis: 0,
            value: 0x1234,
        },
        false,
        0,
        |_| {},
    );

    // The record landed in the session slot's ring (axis value LE at
    // bytes 4..6, axis type/number) — NOT in the decoy's, which a
    // class-based registry lookup would have chosen.
    // SAFETY: buf is a kernel stack buffer of 8 bytes; slot 0 in range.
    let r = unsafe { js_read(0, buf.as_mut_ptr(), 8) };
    assert_eq!(r, Ok(8));
    assert_eq!(&buf[4..6], &0x1234i16.to_le_bytes());
    assert_eq!(buf[6], JS_EVENT_AXIS);
    assert_eq!(buf[7], 0);

    // Free the decoy registry slot again for any later pass.
    let mut inactive = decoy;
    inactive.active = false;
    assert_eq!(dev::set_slot(1, inactive), Ok(()));
}

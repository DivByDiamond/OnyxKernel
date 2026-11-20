//! Host tests for the /dev/jsN joydev rings: inode mapping, push→read
//! roundtrip with whole-record delivery, EAGAIN-not-EOF, oldest-drop
//! overflow, and the no-session namespace gate. Ring-touching tests
//! each own a slot distinct from 0 (the event-drain routing test owns
//! ring 0) and run sequentially inside, so parallel test harts cannot
//! interfere — the dev/tests.rs registry pattern. Record-layout,
//! init-burst and claim-hook tests live in `record_tests.rs`.

use super::rec::{JS_EVENT_AXIS, JS_REC_BYTES};
use super::{
    DEVFS_JS_BASE_INO, JS_RING_RECS, Ring, is_js_ino, js_claim, js_ino, js_read, push_event,
};
use crate::drivers::bus::usb::dev::MAX_SLOTS;
use crate::drivers::bus::usb::hid::HidEvent;
use onyx_core::errno::{Errno, KResult};

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
    let r = unsafe { js_read(idx, buf.as_mut_ptr(), len) };
    (r, buf)
}

#[test]
fn js_inode_mapping_covers_the_base_range() {
    assert_eq!(is_js_ino(DEVFS_JS_BASE_INO), Some(0));
    assert_eq!(is_js_ino(DEVFS_JS_BASE_INO + 3), Some(3));
    // One past the last slot and one below the base resolve to nothing.
    assert_eq!(is_js_ino(DEVFS_JS_BASE_INO + MAX_SLOTS as u32), None);
    assert_eq!(is_js_ino(DEVFS_JS_BASE_INO - 1), None);
    assert_eq!(js_ino(0), DEVFS_JS_BASE_INO);
    assert_eq!(js_ino(3), DEVFS_JS_BASE_INO + 3);
}

/// Slot 2: whole-record delivery, EAGAIN on empty, no record without 8
/// bytes of room, and the no-session open hook inventing nothing.
#[test]
fn read_delivers_whole_records_and_empty_is_eagain() {
    reset(2);
    let (r, _) = read_bytes(2, 8);
    assert_eq!(r, Err(Errno::Again)); // empty: EAGAIN, never a fake EOF

    push_event(2, HidEvent::Axis { axis: 0, value: 1 }, false);
    push_event(2, HidEvent::Axis { axis: 0, value: 2 }, false);
    push_event(2, HidEvent::Axis { axis: 0, value: 3 }, false);

    // 20 bytes fit two whole records; the trailing 4 bytes stay.
    let (r, b) = read_bytes(2, 20);
    assert_eq!(r, Ok(16));
    assert_eq!(&b[4..6], &[1, 0]); // record 0 value
    assert_eq!(&b[12..14], &[2, 0]); // record 1 value
    let (r, b) = read_bytes(2, 8);
    assert_eq!(r, Ok(8));
    assert_eq!(&b[4..6], &[3, 0]);

    // A buffer too small for one record delivers nothing either.
    push_event(2, HidEvent::Axis { axis: 0, value: 4 }, false);
    let (r, _) = read_bytes(2, 4);
    assert_eq!(r, Err(Errno::Again));
    let (r, b) = read_bytes(2, 8);
    assert_eq!(r, Ok(8));
    assert_eq!(&b[4..6], &[4, 0]);
    let (r, _) = read_bytes(2, 8);
    assert_eq!(r, Err(Errno::Again));

    // Open hook with no attached session (host tests): the claim must
    // NOT latch (no baseline, no reader to release it — a latched
    // claim would silence the cursor mapper) and the burst pushes
    // nothing.
    let guard = crate::drivers::input::event::CLAIM_TEST_GUARD
        .lock()
        .unwrap();
    js_claim(2);
    assert!(!crate::drivers::input::event::stick_cursor::claimed());
    let (r, _) = read_bytes(2, 8);
    assert_eq!(r, Err(Errno::Again));
    drop(guard);
}

/// Slot 3: overflow drops the OLDEST records — the newest 64 survive.
#[test]
fn overflow_drops_the_oldest_records() {
    reset(3);
    for i in 0..(JS_RING_RECS as i16 + 3) {
        push_event(3, HidEvent::Axis { axis: 0, value: i }, false);
    }
    let want = (JS_RING_RECS * JS_REC_BYTES) as u32;
    let (r, b) = read_bytes(3, want);
    assert_eq!(r, Ok(want));
    // Oldest survivor is push #3 (the first three were dropped).
    let v = 3i16.to_le_bytes();
    assert_eq!(&b[4..6], &[v[0], v[1]]);
    assert_eq!(b[6], JS_EVENT_AXIS);
    // Newest survivor is push #66.
    let v = 66i16.to_le_bytes();
    let at = (JS_RING_RECS - 1) * JS_REC_BYTES;
    assert_eq!(&b[at + 4..at + 6], &[v[0], v[1]]);
}

#[test]
fn out_of_range_slots_are_refused_not_panicked() {
    push_event(MAX_SLOTS, HidEvent::Axis { axis: 0, value: 0 }, false);
    let (r, _) = read_bytes(MAX_SLOTS, 8);
    assert_eq!(r, Err(Errno::NoEnt));
}

#[test]
fn js_nodes_do_not_exist_without_an_attached_session() {
    // Host tests never attach a HID session: the namespace gate must
    // refuse /dev/jsN instead of exposing a dead node.
    assert_eq!(crate::fs::devfs::lookup(b"js0"), Err(Errno::NoEnt));
    assert_eq!(crate::fs::devfs::lookup(b"jsX"), Err(Errno::NoEnt));
}

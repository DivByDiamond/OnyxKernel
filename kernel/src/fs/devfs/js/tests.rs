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
use crate::fs::vfs::{Fs, VfsFd};
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
    assert!(!crate::drivers::input::event::stick_cursor::claimed(2));
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

/// Regression: a short name ("js0") written after a longer one ("blk0")
/// into the same readdir buffer kept the stale tail and listed "js00".
#[test]
fn readdir_names_are_nul_terminated_over_a_dirty_buffer() {
    let mut buf = *b"blk0xxxx";
    super::super::copy_name(b"js0", buf.as_mut_ptr(), buf.len());
    assert_eq!(&buf[..4], b"js0\0");
    // A full-width name needs no terminator and must not overrun.
    let mut tight = *b"abc";
    super::super::copy_name(b"js1", tight.as_mut_ptr(), tight.len());
    assert_eq!(&tight, b"js1");
}

fn fd(fs: Fs, ino: u32, used: bool) -> VfsFd {
    VfsFd {
        fs,
        ino,
        used,
        ..VfsFd::default()
    }
}

#[test]
fn resume_finds_only_live_devfs_js_fds() {
    use super::resume::held_slots;
    let fds = [
        fd(Fs::Devfs, js_ino(1), true),
        fd(Fs::Devfs, js_ino(1), true),  // dup/second open: one slot
        fd(Fs::Devfs, js_ino(3), false), // closed
        fd(Fs::Onyx, js_ino(2), true),   // same number, other fs
        fd(Fs::Devfs, DEVFS_JS_BASE_INO - 1, true), // not a js node
    ];
    let held = held_slots(&fds);
    assert!(held[1]);
    assert_eq!(held.iter().filter(|h| **h).count(), 1);
}

/// SIGCONT re-sync: stale history is dropped and only the fresh init
/// burst (current state) is left, in axes-then-buttons order.
#[test]
fn resync_drops_the_stale_tail_and_leaves_one_init_burst() {
    use crate::drivers::bus::usb::hid::Baseline;
    use crate::fs::devfs::js::rec::JsRec;
    let mut ring = Ring::new();
    for v in 0..(JS_RING_RECS as i16 + 10) {
        ring.push(JsRec::from_event(
            HidEvent::Axis { axis: 0, value: v },
            false,
            0,
        ));
    }
    let base = Baseline {
        axes: [9, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        axis_n: 1,
        buttons: 0,
        button_n: 1,
    };
    super::burst_into(&mut ring, &base, 0, true);
    assert_eq!(ring.len, 2);
    let axis = ring.pop().unwrap().bytes();
    assert_eq!(axis[4], 9, "burst axis value, not the stale tail");
    assert_eq!(axis[6], super::JS_EVENT_AXIS | super::JS_EVENT_INIT);
    let button = ring.pop().unwrap().bytes();
    assert_eq!(button[6], super::JS_EVENT_BUTTON | super::JS_EVENT_INIT);
}

/// Without flush the burst appends (the open path): history is kept.
#[test]
fn open_burst_appends_without_dropping_history() {
    use crate::drivers::bus::usb::hid::Baseline;
    use crate::fs::devfs::js::rec::JsRec;
    let mut ring = Ring::new();
    ring.push(JsRec::from_event(
        HidEvent::Axis { axis: 0, value: 1 },
        false,
        0,
    ));
    let base = Baseline {
        axes: [0; 16],
        axis_n: 1,
        buttons: 0,
        button_n: 0,
    };
    super::burst_into(&mut ring, &base, 0, false);
    assert_eq!(ring.len, 2);
    assert_eq!(ring.pop().unwrap().bytes()[6], super::JS_EVENT_AXIS);
}

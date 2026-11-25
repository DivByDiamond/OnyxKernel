//! SIGCONT hand-over of /dev/jsN input ownership.
//!
//! A SIGSTOPped consumer keeps its js fds (and so its claim: the cursor
//! mapper stays off, the ring keeps filling with the last `JS_RING_RECS`
//! records, oldest dropped). When the desktop continues it, the resumed
//! process becomes the owner of every js node it holds and each of those
//! rings is re-synced: stale history dropped, a fresh `JS_EVENT_INIT`
//! burst from the live baseline pushed. A reader therefore never replays
//! a half-stale tail, and — because the burst carries the CURRENT axis
//! and button state — never misses a state change that happened while it
//! was stopped. Consumers must treat `JS_EVENT_INIT` records as state,
//! not as noise to skip.

use super::{is_js_ino, push_burst};
use crate::drivers::bus::usb::dev::MAX_SLOTS;
use crate::drivers::bus::usb::hid::baseline;
use crate::drivers::input::event::stick_cursor;
use crate::fs::vfs::{Fs, VfsFd};

/// Which js slots does this fd table hold open?
pub(crate) fn held_slots(fds: &[VfsFd]) -> [bool; MAX_SLOTS] {
    let mut held = [false; MAX_SLOTS];
    for fd in fds.iter().filter(|f| f.used && f.fs == Fs::Devfs) {
        if let Some(slot) = is_js_ino(fd.ino) {
            held[slot] = true;
        }
    }
    held
}

/// Re-sync every js node held by the resumed process `pid`/`fds`;
/// returns how many rings were re-seeded. A slot whose session went
/// away while the process slept has no baseline and is skipped.
pub fn js_resume(pid: u32, fds: &[VfsFd]) -> usize {
    let mut n = 0;
    for (idx, _) in held_slots(fds).iter().enumerate().filter(|(_, h)| **h) {
        if let Some(base) = baseline(idx) {
            stick_cursor::set_owner(idx, pid);
            push_burst(idx, &base, true);
            n += 1;
        }
    }
    n
}

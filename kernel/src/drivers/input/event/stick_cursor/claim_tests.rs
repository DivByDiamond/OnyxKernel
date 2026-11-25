//! Per-slot claim counters and the claim-gated mapping (regression for
//! the former single global claim flag).

use super::*;
use crate::drivers::bus::usb::dev::MAX_SLOTS;
use crate::drivers::input::event::forward_stick;

/// Regression: the claim used to be one global flag, so closing any
/// js fd re-enabled the cursor mapper for every stick.
#[test]
fn claims_are_per_slot_and_counted() {
    let _claim = crate::drivers::input::event::CLAIM_TEST_GUARD
        .lock()
        .unwrap();
    claim(0);
    claim(1);
    assert!(claimed(0) && claimed(1) && !claimed(2));
    // Closing js0 leaves js1 claimed.
    release(0);
    assert!(!claimed(0) && claimed(1));
    // Two readers of one node: the first close keeps the claim.
    claim(1);
    release(1);
    assert!(claimed(1));
    release(1);
    assert!(!claimed(1));
    // Underflow and out-of-range are no-ops.
    release(1);
    assert!(!claimed(1));
    claim(MAX_SLOTS);
    release(MAX_SLOTS);
    assert!(!claimed(MAX_SLOTS));
}

/// A claimed stick stops mapping, an unclaimed neighbour keeps driving
/// the cursor.
#[test]
fn claim_gates_the_mapping_per_stick() {
    let _claim = crate::drivers::input::event::CLAIM_TEST_GUARD
        .lock()
        .unwrap();
    let mut out = std::vec::Vec::new();
    let ev = HidEvent::Axis {
        axis: 0,
        value: 20000,
    };
    claim(0);
    forward_stick(ev, 0, |e| out.push(e));
    assert_eq!(out.len(), 1); // raw only
    out.clear();
    forward_stick(ev, 1, |e| out.push(e));
    assert_eq!(out.len(), 2); // raw + mapped
    release(0);
}

/// The owner follows the last opener, survives a non-final close and is
/// dropped with the last claim (a SIGSTOP never touches any of this).
#[test]
fn owner_is_set_by_open_or_resume_and_cleared_with_the_last_claim() {
    let _claim = crate::drivers::input::event::CLAIM_TEST_GUARD
        .lock()
        .unwrap();
    assert_eq!(owner(3), 0);
    claim(3);
    set_owner(3, 100);
    claim(3); // a second app opens the same node
    set_owner(3, 200);
    assert_eq!(owner(3), 200);
    release(3);
    assert!(claimed(3));
    assert_eq!(owner(3), 200, "claim kept, owner kept while a fd remains");
    set_owner(3, 100); // SIGCONT hands it back
    assert_eq!(owner(3), 100);
    release(3);
    assert!(!claimed(3));
    assert_eq!(owner(3), 0);
    set_owner(MAX_SLOTS, 1);
    assert_eq!(owner(MAX_SLOTS), 0);
}

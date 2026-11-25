//! Host tests for the stick → cursor mapper: the pure `map()` table,
//! the claim flag, and the claim-gated dispatch integration.

use super::*;
use crate::drivers::bus::usb::hid::HidEvent;
use crate::drivers::input::event::{dispatch, forward_stick, on_event};
use crate::drivers::input::mouse;
use crate::drivers::input::{Event, MouseButton};

#[test]
fn dead_zone_gates_motion_and_other_axes_are_ignored() {
    assert_eq!(
        map(HidEvent::Axis {
            axis: 0,
            value: 2000
        }),
        None
    );
    assert_eq!(
        map(HidEvent::Axis {
            axis: 0,
            value: -2000
        }),
        None
    );
    assert_eq!(
        map(HidEvent::Axis {
            axis: 1,
            value: 2999
        }),
        None
    );
    assert_eq!(
        map(HidEvent::Axis {
            axis: 2,
            value: 20000
        }),
        None
    );
    // Exactly at the dead-zone edge the gate opens; the gain itself
    // only produces motion from |value| >= 2^GAIN_SHIFT (4096).
    assert_eq!(
        map(HidEvent::Axis {
            axis: 0,
            value: DEAD_ZONE
        }),
        Some(Event::MouseRel { dx: 0, dy: 0 })
    );
}

#[test]
fn x_axis_uses_arithmetic_gain_so_negative_moves_left() {
    assert_eq!(
        map(HidEvent::Axis {
            axis: CURSOR_AXIS_X,
            value: 20000
        }),
        Some(Event::MouseRel { dx: 4, dy: 0 })
    );
    // Arithmetic shift floors: -20000 >> 12 is -5, not -4 (the task
    // brief's "-4" is an erratum; -16384 is the exact -4 case).
    assert_eq!(
        map(HidEvent::Axis {
            axis: CURSOR_AXIS_X,
            value: -20000
        }),
        Some(Event::MouseRel { dx: -5, dy: 0 })
    );
    assert_eq!(
        map(HidEvent::Axis {
            axis: CURSOR_AXIS_X,
            value: -16384
        }),
        Some(Event::MouseRel { dx: -4, dy: 0 })
    );
}

#[test]
fn y_axis_negative_value_moves_up() {
    // Screen Y grows down: mouse::apply integrates +dy downward, so
    // stick-up (negative HID value) maps to negative dy = cursor up.
    assert_eq!(
        map(HidEvent::Axis {
            axis: CURSOR_AXIS_Y,
            value: -16384
        }),
        Some(Event::MouseRel { dx: 0, dy: -4 })
    );
    assert_eq!(
        map(HidEvent::Axis {
            axis: CURSOR_AXIS_Y,
            value: 20000
        }),
        Some(Event::MouseRel { dx: 0, dy: 4 })
    );
}

#[test]
fn button_zero_is_left_click_and_other_buttons_map_to_nothing() {
    assert_eq!(
        map(HidEvent::Button {
            index: CURSOR_BUTTON,
            down: true
        }),
        Some(Event::MouseButton {
            btn: MouseButton::Left,
            down: true
        })
    );
    assert_eq!(
        map(HidEvent::Button {
            index: CURSOR_BUTTON,
            down: false
        }),
        Some(Event::MouseButton {
            btn: MouseButton::Left,
            down: false
        })
    );
    assert_eq!(
        map(HidEvent::Button {
            index: 3,
            down: true
        }),
        None
    );
}

static HITS: std::sync::Mutex<std::vec::Vec<Event>> = std::sync::Mutex::new(std::vec::Vec::new());

fn record(ev: Event) {
    HITS.lock().unwrap().push(ev);
}

#[test]
fn unclaimed_sticks_drive_the_cursor_and_claim_stops_only_the_mapping() {
    // The tests mutating the claim flag (this one and the devfs js
    // hooks test) serialize on CLAIM_TEST_GUARD: parallel harts cannot
    // interleave claim/release calls between an assert pair.
    let _claim = crate::drivers::input::event::CLAIM_TEST_GUARD
        .lock()
        .unwrap();
    // Everything else this test mutates stays private to it.
    claim(1);
    assert!(claimed(1));
    release(1);
    assert!(!claimed(1));

    on_event(record);
    let (x0, _, _) = mouse::snapshot();

    // Unclaimed: raw Stick out first, then the mapped MouseRel. Slot 1
    // in every forward_stick call: that half of the drain never
    // touches a /dev/jsN ring, so this test owns only the
    // cursor/handler state. (Ring ROUTING has its own drain-level
    // test, event/tests.rs.)
    forward_stick(
        HidEvent::Axis {
            axis: 0,
            value: 20000,
        },
        1,
        dispatch,
    );
    let (x1, _, _) = mouse::snapshot();
    assert_eq!(x1, x0 + 4);
    assert!(matches!(
        HITS.lock().unwrap()[..],
        [
            Event::Stick {
                axis: 0,
                value: 20000
            },
            Event::MouseRel { dx: 4, dy: 0 }
        ]
    ));

    // A raw Stick dispatch alone never moves the cursor — motion only
    // comes from the mapped event.
    dispatch(Event::Stick {
        axis: 0,
        value: 20000,
    });
    assert_eq!(mouse::snapshot().0, x1);

    // Unclaimed button 0 feeds the left-button edge history.
    forward_stick(
        HidEvent::Button {
            index: 0,
            down: true,
        },
        1,
        dispatch,
    );
    assert_eq!(mouse::snapshot().2, 1);
    forward_stick(
        HidEvent::Button {
            index: 0,
            down: false,
        },
        1,
        dispatch,
    );
    assert_eq!(mouse::snapshot().2, 0);

    // Claimed: the mapper stops, the raw stream keeps flowing.
    claim(1);
    forward_stick(
        HidEvent::Axis {
            axis: 0,
            value: 20000,
        },
        1,
        dispatch,
    );
    let (x2, _, buttons) = mouse::snapshot();
    assert_eq!(x2, x1);
    assert_eq!(buttons, 0);
    assert!(matches!(
        HITS.lock().unwrap()[..],
        [
            ..,
            Event::Stick {
                axis: 0,
                value: 20000
            }
        ]
    ));

    forward_stick(
        HidEvent::Button {
            index: 0,
            down: true,
        },
        1,
        dispatch,
    );
    assert_eq!(mouse::snapshot().2, 0);

    // Leave the counter released (unclaimed is the default state).
    release(1);
}

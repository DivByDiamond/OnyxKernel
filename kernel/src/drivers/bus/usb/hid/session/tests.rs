//! Host tests for the HID session's diff core: scripted reports over
//! the task-6 joystick fixture drive `apply_report` directly — the
//! first-report init burst, the axis/button diff, and the short-report
//! drop. No MMIO anywhere; the attach/poll wrappers stay
//! boot-probe/100 Hz MMIO paths.

use super::MAX_SLOTS;
use super::diff::{SlotSession, apply_report};
use crate::drivers::usb::hid::report::fixtures::joystick_desc;
use crate::drivers::usb::hid::{HidEvent, MAX_FIELDS, parse_report_desc};
use std::vec::Vec;

/// Joystick button 3 (usage 4): report bit 84 + 3.
const BTN3: u16 = 1 << 3;

/// A session over the joystick fixture table (panic on fixture parse —
/// tests only, the `parse_ok` precedent).
fn stick() -> SlotSession {
    match parse_report_desc(&joystick_desc()) {
        Ok(t) => SlotSession::from_table(t),
        Err(e) => panic!("fixture parse: {e:?}"),
    }
}

/// Apply one report and collect its events.
fn events(s: &mut SlotSession, report: &[u8]) -> Vec<(HidEvent, bool)> {
    let mut out = [(HidEvent::Axis { axis: 0, value: 0 }, false); MAX_FIELDS];
    let n = apply_report(s, report, &mut out);
    out[..n].to_vec()
}

/// Joystick report: 13 bytes — five i16 LE axes, 4-bit hat, 16 button
/// bits (report bits 84..100).
fn stick_report(x: i16, hat: u8, buttons: u16) -> [u8; 13] {
    let mut r = [0u8; 13];
    r[0..2].copy_from_slice(&x.to_le_bytes());
    r[10] = (hat & 0x0F) | (((buttons & 0x0F) as u8) << 4);
    r[11] = ((buttons >> 4) & 0xFF) as u8;
    r[12] = ((buttons >> 12) & 0x0F) as u8;
    r
}

#[test]
fn first_report_emits_every_field_with_init_flag() {
    let mut s = stick();
    let evs = events(&mut s, &stick_report(0, 0, 0));
    // 7 axes (X/Y/Z/Rz/Slider/hat X/hat Y) + 16 buttons = 23 fields,
    // every one of them an init event (komnata JS_EVENT_INIT analog),
    // buttons included so consumers learn the released state too.
    assert_eq!(evs.len(), 23);
    assert!(evs.iter().all(|(_, init)| *init));
    assert_eq!(evs[0], (HidEvent::Axis { axis: 0, value: 0 }, true));
    assert_eq!(
        evs[7],
        (
            HidEvent::Button {
                index: 0,
                down: false
            },
            true
        )
    );
    assert_eq!(
        evs[22],
        (
            HidEvent::Button {
                index: 15,
                down: false
            },
            true
        )
    );
}

#[test]
fn repeat_of_same_report_emits_nothing() {
    let mut s = stick();
    assert_eq!(events(&mut s, &stick_report(0, 0, 0)).len(), 23);
    assert!(events(&mut s, &stick_report(0, 0, 0)).is_empty());
    // A report with a button already held is also a repeat after the
    // baseline caught it: no edge, no event.
    let mut s = stick();
    assert_eq!(events(&mut s, &stick_report(0, 0, BTN3)).len(), 23);
    assert!(events(&mut s, &stick_report(0, 0, BTN3)).is_empty());
}

#[test]
fn moved_axis_emits_exactly_one_event() {
    let mut s = stick();
    events(&mut s, &stick_report(0, 0, 0));
    // X moves 0 → 100: one event, no init flag.
    let evs = events(&mut s, &stick_report(100, 0, 0));
    assert_eq!(
        evs,
        vec![(
            HidEvent::Axis {
                axis: 0,
                value: 100
            },
            false
        )]
    );
    // Hold position: silent again.
    assert!(events(&mut s, &stick_report(100, 0, 0)).is_empty());
    // Negative move.
    let evs = events(&mut s, &stick_report(-32768, 0, 0));
    assert_eq!(
        evs,
        vec![(
            HidEvent::Axis {
                axis: 0,
                value: -32768
            },
            false
        )]
    );
}

#[test]
fn button_press_and_release_emit_edges() {
    let mut s = stick();
    events(&mut s, &stick_report(0, 0, 0));
    // Press: down edge.
    let evs = events(&mut s, &stick_report(0, 0, BTN3));
    assert_eq!(
        evs,
        vec![(
            HidEvent::Button {
                index: 3,
                down: true
            },
            false
        )]
    );
    // Hold: no edge.
    assert!(events(&mut s, &stick_report(0, 0, BTN3)).is_empty());
    // Release: up edge.
    let evs = events(&mut s, &stick_report(0, 0, 0));
    assert_eq!(
        evs,
        vec![(
            HidEvent::Button {
                index: 3,
                down: false
            },
            false
        )]
    );
}

#[test]
fn hat_move_moves_two_axes() {
    let mut s = stick();
    events(&mut s, &stick_report(0, 0, 0));
    // Hat 2 = NE: hat-X 0 → 1, hat-Y 0 → -1; the raw nibble itself is
    // not an event source.
    let evs = events(&mut s, &stick_report(0, 2, 0));
    assert_eq!(
        evs,
        vec![
            (HidEvent::Axis { axis: 5, value: 1 }, false),
            (HidEvent::Axis { axis: 6, value: -1 }, false),
        ]
    );
    // Back to null state: both axes return to 0.
    let evs = events(&mut s, &stick_report(0, 0, 0));
    assert_eq!(
        evs,
        vec![
            (HidEvent::Axis { axis: 5, value: 0 }, false),
            (HidEvent::Axis { axis: 6, value: 0 }, false),
        ]
    );
}

#[test]
fn short_report_is_dropped_and_baseline_kept() {
    let mut s = stick();
    // Baseline with X=100, button 3 held.
    events(&mut s, &stick_report(100, 0, BTN3));
    // A report shorter than the table's byte extent decodes nothing:
    // dropped silently (a corrupt endpoint must not spin or spam) and
    // the baseline stays untouched.
    assert_eq!(events(&mut s, &[0u8; 4]).len(), 0);
    // The next full report diffs against the OLD baseline: an
    // unchanged repeat stays silent.
    assert!(events(&mut s, &stick_report(100, 0, BTN3)).is_empty());
    // And the press edge still emits once the button actually moves.
    let evs = events(&mut s, &stick_report(100, 0, 0));
    assert_eq!(
        evs,
        vec![(
            HidEvent::Button {
                index: 3,
                down: false
            },
            false
        )]
    );
}

#[test]
fn simultaneous_axis_and_button_changes_batch_one_report() {
    let mut s = stick();
    events(&mut s, &stick_report(0, 0, 0));
    let evs = events(&mut s, &stick_report(-500, 5, BTN3));
    assert_eq!(
        evs,
        vec![
            (
                HidEvent::Axis {
                    axis: 0,
                    value: -500
                },
                false
            ),
            (HidEvent::Axis { axis: 6, value: 1 }, false), // hat 5 = S
            (
                HidEvent::Button {
                    index: 3,
                    down: true
                },
                false
            ),
        ]
    );
}

#[test]
fn snapshot_reports_baseline_state_and_counts() {
    let mut s = stick();
    events(&mut s, &stick_report(-500, 0, BTN3));
    let b = s.snapshot().expect("attached session snapshots");
    // The fixture: 7 axes (X/Y/Z/Rz/Slider/hat X/hat Y), 16 buttons.
    assert_eq!(b.axis_n, 7);
    assert_eq!(b.axes[0], -500);
    assert_eq!(b.button_n, 16);
    assert_eq!(b.buttons, BTN3 as u32);
}

#[test]
fn unattached_session_snapshots_none() {
    assert!(SlotSession::empty().snapshot().is_none());
}

#[test]
fn scan_rotates_from_the_cursor_and_visits_every_slot_once() {
    let order: Vec<usize> = (0..MAX_SLOTS).map(|s| super::scan_slot(2, s)).collect();
    assert_eq!(order, [2, 3, 0, 1]);
}

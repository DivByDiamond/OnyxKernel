//! Shared test fixtures: raw report-descriptor item builders (HID
//! 1.11 §6.2.2) and the two brief fixtures.

use super::super::{
    GD_HAT, GD_RZ, GD_SLIDER, GD_X, GD_Y, GD_Z, HidTable, PAGE_BUTTON, PAGE_GENERIC_DESKTOP,
};
use super::parse_report_desc;
use std::vec::Vec;

/// Input flags: Data, Variable, Absolute (§6.2.2.4).
pub(crate) const INPUT_DVA: u8 = 0x02;
/// Collection kinds (§6.2.2.4): Physical, Application.
pub(crate) const COLL_PHYSICAL: u8 = 0x00;
pub(crate) const COLL_APPLICATION: u8 = 0x01;
/// Non-axis Generic Desktop collection usages (Usage Tables §4):
/// Joystick 0x04, Pointer 0x01 — bits without a decoded field.
pub(crate) const USAGE_JOYSTICK: u16 = 0x04;
pub(crate) const USAGE_POINTER: u16 = 0x01;

pub(crate) fn page(p: u16, d: &mut Vec<u8>) {
    d.extend_from_slice(&[0x05, p as u8]);
}
pub(crate) fn usage(u: u16, d: &mut Vec<u8>) {
    d.extend_from_slice(&[0x09, u as u8]);
}
pub(crate) fn usage_min(u: u16, d: &mut Vec<u8>) {
    d.extend_from_slice(&[0x19, u as u8]);
}
pub(crate) fn usage_max(u: u16, d: &mut Vec<u8>) {
    d.extend_from_slice(&[0x29, u as u8]);
}

/// Logical Minimum: 1-byte item when it fits, else the 2-byte form
/// (0x16) the ±32767 axes need (§6.2.2.2 size encoding).
pub(crate) fn logical_min(v: i32, d: &mut Vec<u8>) {
    if let Ok(b) = i8::try_from(v) {
        d.extend_from_slice(&[0x15, b as u8]);
    } else {
        d.extend_from_slice(&[0x16]);
        d.extend_from_slice(&(v as i16).to_le_bytes());
    }
}

/// Logical Maximum: same size rule with the 0x25/0x26 items.
pub(crate) fn logical_max(v: i32, d: &mut Vec<u8>) {
    if let Ok(b) = i8::try_from(v) {
        d.extend_from_slice(&[0x25, b as u8]);
    } else {
        d.extend_from_slice(&[0x26]);
        d.extend_from_slice(&(v as i16).to_le_bytes());
    }
}

pub(crate) fn report_size(bits: u8, d: &mut Vec<u8>) {
    d.extend_from_slice(&[0x75, bits]);
}
pub(crate) fn report_count(n: u8, d: &mut Vec<u8>) {
    d.extend_from_slice(&[0x95, n]);
}
pub(crate) fn input(f: u8, d: &mut Vec<u8>) {
    d.extend_from_slice(&[0x81, f]);
}
pub(crate) fn output(f: u8, d: &mut Vec<u8>) {
    d.extend_from_slice(&[0x91, f]);
}
pub(crate) fn collection(kind: u8, d: &mut Vec<u8>) {
    d.extend_from_slice(&[0xA1, kind]);
}
pub(crate) fn end_collection(d: &mut Vec<u8>) {
    d.push(0xC0);
}

/// Parse helper: the table, or panic with the error (tests only).
pub(crate) fn parse_ok(desc: &[u8]) -> HidTable {
    match parse_report_desc(desc) {
        Ok(t) => t,
        Err(e) => panic!("expected Ok, got {e:?} for {desc:02x?}"),
    }
}

/// Brief fixture A — T.16000M-style joystick: four 16-bit signed axes
/// (X/Y/Z/Rz, logical ±32767), Slider, 4-bit hat (0..7), 16 buttons
/// via a Usage Minimum/Maximum range (§6.2.2.8, as the spec's own
/// Appendix D.1 joystick does).
pub(crate) fn joystick_desc() -> Vec<u8> {
    let mut d = Vec::new();
    page(PAGE_GENERIC_DESKTOP, &mut d);
    usage(USAGE_JOYSTICK, &mut d);
    collection(COLL_APPLICATION, &mut d);
    usage(USAGE_POINTER, &mut d);
    collection(COLL_PHYSICAL, &mut d);
    for u in [GD_X, GD_Y, GD_Z, GD_RZ, GD_SLIDER] {
        usage(u, &mut d);
        logical_min(-32767, &mut d);
        logical_max(32767, &mut d);
        report_size(16, &mut d);
        report_count(1, &mut d);
        input(INPUT_DVA, &mut d);
    }
    usage(GD_HAT, &mut d);
    logical_min(0, &mut d);
    logical_max(7, &mut d);
    report_size(4, &mut d);
    report_count(1, &mut d);
    input(INPUT_DVA, &mut d);
    page(PAGE_BUTTON, &mut d);
    usage_min(1, &mut d);
    usage_max(16, &mut d);
    logical_min(0, &mut d);
    logical_max(1, &mut d);
    report_size(1, &mut d);
    report_count(16, &mut d);
    input(INPUT_DVA, &mut d);
    end_collection(&mut d);
    end_collection(&mut d);
    d
}

/// Brief fixture B — minimal tablet: two unsigned 16-bit absolute axes
/// (X/Y via two Usage tags on one Input, §6.2.2.8) + 2 buttons.
pub(crate) fn tablet_desc() -> Vec<u8> {
    let mut d = Vec::new();
    page(PAGE_GENERIC_DESKTOP, &mut d);
    usage(USAGE_POINTER, &mut d);
    collection(COLL_APPLICATION, &mut d);
    usage(GD_X, &mut d);
    usage(GD_Y, &mut d);
    logical_min(0, &mut d);
    logical_max(32767, &mut d);
    report_size(16, &mut d);
    report_count(2, &mut d);
    input(INPUT_DVA, &mut d);
    page(PAGE_BUTTON, &mut d);
    usage_min(1, &mut d);
    usage_max(2, &mut d);
    logical_min(0, &mut d);
    logical_max(1, &mut d);
    report_size(1, &mut d);
    report_count(2, &mut d);
    input(INPUT_DVA, &mut d);
    end_collection(&mut d);
    d
}

/// Fixture C — QEMU `usb-tablet` (dev-hid.c qemu_tablet_hid_report_
/// descriptor), byte-for-byte: the descriptor the live Task-11 smoke
/// greps a stick session for. Carries the Physical Min/Max (0x35/0x46)
/// items the parser used to reject, plus a signed relative Wheel.
pub(crate) fn qemu_tablet_desc() -> Vec<u8> {
    vec![
        0x05, 0x01, 0x09, 0x02, 0xa1, 0x01, 0x09, 0x01, 0xa1, 0x00, 0x05, 0x09, 0x19, 0x01, 0x29,
        0x05, 0x15, 0x00, 0x25, 0x01, 0x95, 0x05, 0x75, 0x01, 0x81, 0x02, 0x95, 0x01, 0x75, 0x03,
        0x81, 0x01, 0x05, 0x01, 0x09, 0x30, 0x09, 0x31, 0x15, 0x00, 0x26, 0xff, 0x7f, 0x35, 0x00,
        0x46, 0xff, 0x7f, 0x75, 0x10, 0x95, 0x02, 0x81, 0x02, 0x05, 0x01, 0x09, 0x38, 0x15, 0x81,
        0x25, 0x7f, 0x35, 0x00, 0x45, 0x00, 0x75, 0x08, 0x95, 0x01, 0x81, 0x06, 0xc0, 0xc0,
    ]
}

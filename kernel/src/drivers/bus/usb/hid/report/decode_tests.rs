//! Decoder tests: joystick/tablet reports, hat positions (brief
//! fixtures A/B).

use super::super::{FieldKind, MAX_FIELDS};
use super::decode_report;
use super::fixtures::*;

#[test]
fn joystick_decode() {
    let t = parse_ok(&joystick_desc());
    // 13-byte report (100 bits): X=-32768, Y=1, Z=32767, Rz=-1,
    // slider=0x1234, hat=5 (S), buttons 0..15 pattern.
    let report = [
        0x00, 0x80, 0x01, 0x00, 0xFF, 0x7F, 0xFF, 0xFF, 0x34, 0x12, 0x55, 0xFF, 0x0F,
    ];
    let mut out = [(FieldKind::Axis(0), 0i16); MAX_FIELDS];
    assert_eq!(decode_report(&t, &report, &mut out), 23);
    assert_eq!(out[0], (FieldKind::Axis(0), -32768)); // brief: LE i16, -32768-tolerant
    assert_eq!(out[1], (FieldKind::Axis(1), 1));
    assert_eq!(out[2], (FieldKind::Axis(2), 32767));
    assert_eq!(out[3], (FieldKind::Axis(3), -1));
    assert_eq!(out[4], (FieldKind::Axis(4), 0x1234));
    assert_eq!(out[5], (FieldKind::Axis(5), 0)); // hat=5: x=0
    assert_eq!(out[6], (FieldKind::Axis(6), 1)); // hat=5: y=1 (brief)
    assert_eq!(out[7], (FieldKind::Button(0), 1));
    assert_eq!(out[8], (FieldKind::Button(1), 0));
    assert_eq!(out[9], (FieldKind::Button(2), 1));
    assert_eq!(out[10], (FieldKind::Button(3), 0));
    assert_eq!(out[11], (FieldKind::Button(4), 1));
    assert_eq!(out[22], (FieldKind::Button(15), 1));
    // A report shorter than the table's extent decodes nothing.
    assert_eq!(decode_report(&t, &[0u8; 4], &mut out), 0);
}

#[test]
fn hat_positions_decode() {
    let t = parse_ok(&joystick_desc());
    for (hat, dx, dy) in [
        (0u8, 0i16, 0i16), // null state (0, per the brief)
        (2, 1, -1),        // NE
        (5, 0, 1),         // S — the brief's case
        (7, -1, 0),        // W
        (15, 0, 0),        // null state (15, per the brief)
    ] {
        let mut report = [0u8; 13];
        report[10] = hat;
        let mut out = [(FieldKind::Axis(0), 0i16); MAX_FIELDS];
        assert_eq!(decode_report(&t, &report, &mut out), 23);
        assert_eq!((out[5].1, out[6].1), (dx, dy), "hat {hat}");
    }
}

#[test]
fn tablet_decode() {
    let t = parse_ok(&tablet_desc());
    let report = [0x00, 0x10, 0xFF, 0x7F, 0x02];
    let mut out = [(FieldKind::Axis(0), 0i16); MAX_FIELDS];
    assert_eq!(decode_report(&t, &report, &mut out), 4);
    assert_eq!(out[0], (FieldKind::Axis(0), 0x1000));
    assert_eq!(out[1], (FieldKind::Axis(1), 32767));
    assert_eq!(out[2], (FieldKind::Button(0), 0));
    assert_eq!(out[3], (FieldKind::Button(1), 1));
}

#[test]
fn cross_byte_field_decode() {
    // Y at bits 11..18 (8-bit unsigned) after an X byte and 3 pad bits.
    let mut d = std::vec::Vec::new();
    page(super::super::PAGE_GENERIC_DESKTOP, &mut d);
    usage(super::super::GD_X, &mut d);
    logical_min(0, &mut d);
    logical_max(255, &mut d);
    report_size(8, &mut d);
    report_count(1, &mut d);
    input(INPUT_DVA, &mut d);
    report_size(3, &mut d); // pad: usage-less Input (§6.2.2.9)
    report_count(1, &mut d);
    input(INPUT_DVA, &mut d);
    usage(super::super::GD_Y, &mut d);
    report_size(8, &mut d);
    input(INPUT_DVA, &mut d);
    let t = parse_ok(&d);
    let report = [0x0F, 0xF8, 0x01];
    let mut out = [(FieldKind::Axis(0), 0i16); MAX_FIELDS];
    assert_eq!(decode_report(&t, &report, &mut out), 2);
    assert_eq!(out[0], (FieldKind::Axis(0), 15));
    assert_eq!(out[1], (FieldKind::Axis(1), 63)); // bits 11..18, cross-byte
}

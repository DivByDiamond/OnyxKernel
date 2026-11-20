//! Parser tests: table layout, ids, negatives (brief fixtures A/B).

use super::super::{
    FieldKind, GD_HAT, GD_RX, GD_RY, GD_RZ, GD_SLIDER, GD_X, GD_Y, GD_Z, HatPart, HidError,
    PAGE_BUTTON, PAGE_GENERIC_DESKTOP,
};
use super::fixtures::*;
use super::parse_report_desc;

#[test]
fn joystick_table_layout() {
    let t = parse_ok(&joystick_desc());
    assert_eq!(t.n_fields, 23);
    assert_eq!(t.report_len_bits, 100);
    assert!(super::super::has_axes(&t));
    let axes = [
        (FieldKind::Axis(0), 16u8, 0u16, true, HatPart::NotHat),
        (FieldKind::Axis(1), 16, 16, true, HatPart::NotHat),
        (FieldKind::Axis(2), 16, 32, true, HatPart::NotHat),
        (FieldKind::Axis(3), 16, 48, true, HatPart::NotHat),
        (FieldKind::Axis(4), 16, 64, true, HatPart::NotHat),
        (FieldKind::Axis(5), 4, 80, false, HatPart::HatX),
        (FieldKind::Axis(6), 4, 80, false, HatPart::HatY),
    ];
    for (i, want) in axes.into_iter().enumerate() {
        let f = &t.fields[i];
        assert_eq!(
            (f.kind, f.bits, f.offset_bits, f.signed, f.hat),
            want,
            "field {i}"
        );
    }
    for (b, f) in t.fields.iter().skip(7).take(16).enumerate() {
        assert_eq!(f.kind, FieldKind::Button(b as u8));
        assert_eq!(
            (f.bits, f.offset_bits, f.signed, f.hat),
            (1, 84 + b as u16, false, HatPart::NotHat)
        );
    }
}

#[test]
fn tablet_table_layout() {
    let t = parse_ok(&tablet_desc());
    assert_eq!(t.n_fields, 4);
    assert_eq!(t.report_len_bits, 34);
    assert!(super::super::has_axes(&t));
    assert_eq!(
        (t.fields[0].kind, t.fields[0].signed),
        (FieldKind::Axis(0), false)
    );
    assert_eq!(t.fields[1].kind, FieldKind::Axis(1));
    assert_eq!(t.fields[2].kind, FieldKind::Button(0));
    assert_eq!(t.fields[3].kind, FieldKind::Button(1));
}

#[test]
fn axis_ids_reuse_and_report_offsets() {
    // Ruling 3: a repeated axis usage reuses its first-appearance id.
    let mut d = std::vec::Vec::new();
    page(PAGE_GENERIC_DESKTOP, &mut d);
    for u in [GD_X, GD_Y, GD_X] {
        usage(u, &mut d);
        logical_min(0, &mut d);
        logical_max(255, &mut d);
        report_size(8, &mut d);
        report_count(1, &mut d);
        input(INPUT_DVA, &mut d);
    }
    let t = parse_ok(&d);
    assert_eq!(t.fields[2].kind, FieldKind::Axis(0));

    // Padding (§6.2.2.9) consumes bits without a field; Output reports
    // keep their own offsets, so Y stays input-relative.
    let mut d = std::vec::Vec::new();
    page(PAGE_GENERIC_DESKTOP, &mut d);
    usage(GD_X, &mut d);
    logical_min(0, &mut d);
    logical_max(255, &mut d);
    report_size(8, &mut d);
    report_count(1, &mut d);
    input(INPUT_DVA, &mut d);
    report_size(3, &mut d); // pad: usage-less Input (§6.2.2.9)
    report_count(1, &mut d);
    input(INPUT_DVA, &mut d);
    report_count(2, &mut d);
    output(INPUT_DVA, &mut d); // output report: parsed, dropped
    usage(GD_Y, &mut d);
    report_count(1, &mut d);
    report_size(8, &mut d);
    input(INPUT_DVA, &mut d);
    let t = parse_ok(&d);
    assert_eq!((t.n_fields, t.report_len_bits), (2, 19));
    assert_eq!(t.fields[1].offset_bits, 11);
}

#[test]
fn truncated_descriptor_is_short() {
    // 2-byte Logical Minimum with one data byte missing.
    assert_eq!(
        parse_report_desc(&[0x05, 0x01, 0x09, 0x30, 0x16, 0x01]),
        Err(HidError::Short)
    );
    // Item header promising data past the end.
    assert_eq!(parse_report_desc(&[0x16]), Err(HidError::Short));
    // Long item (0xFE prefix) truncated past its bDataSize.
    assert_eq!(parse_report_desc(&[0xFE, 0x05]), Err(HidError::Short));
}

#[test]
fn too_many_fields_rejected() {
    let mut d = std::vec::Vec::new();
    page(PAGE_GENERIC_DESKTOP, &mut d);
    usage(GD_X, &mut d);
    logical_min(0, &mut d);
    logical_max(255, &mut d);
    report_size(8, &mut d);
    report_count(70, &mut d); // 70 fields > MAX_FIELDS
    input(INPUT_DVA, &mut d);
    assert_eq!(parse_report_desc(&d), Err(HidError::TooManyFields));
}

#[test]
fn unknown_items_rejected() {
    // Main tag 0x00: unassigned (§6.2.2.4 reserved rows).
    assert_eq!(parse_report_desc(&[0x00]), Err(HidError::UnknownItem(0x00)));
    // Global item 0x55: Unit Exponent — an ASSIGNED §6.2.2.7 global
    // item (valid HID), just outside this parser's supported subset.
    // Physical Min/Max (0x35/0x46) used to be the pick here but the
    // QEMU usb-tablet smoke showed real descriptors carry them, so
    // they are parsed-and-dropped globals now (see TAG_PHYSICAL_MIN).
    assert_eq!(
        parse_report_desc(&[0x55, 0x00]),
        Err(HidError::UnknownItem(0x55))
    );
}

#[test]
fn qemu_tablet_descriptor_parses() {
    // The live Task-11 smoke boots QEMU `usb-tablet` and greps the
    // stick-session kinf; this pins the descriptor-side half of that
    // contract to a host test: Physical Min/Max items accepted, the
    // X/Y pair yields axes, and the button field decodes as buttons.
    let t = parse_ok(&qemu_tablet_desc());
    assert!(super::super::has_axes(&t));
    let axes = t
        .fields
        .iter()
        .take(t.n_fields)
        .filter(|f| matches!(f.kind, FieldKind::Axis(_)))
        .count();
    assert_eq!(axes, 2);
    let buttons = t
        .fields
        .iter()
        .take(t.n_fields)
        .filter(|f| matches!(f.kind, FieldKind::Button(_)))
        .count();
    assert_eq!(buttons, 5); // Usage Min/Max 1..5 → one field per button
}

#[test]
fn report_id_rejected() {
    let mut d = std::vec::Vec::new();
    page(PAGE_GENERIC_DESKTOP, &mut d);
    d.extend_from_slice(&[0x85, 0x01]); // Report ID(1) — ruling 4
    usage(GD_X, &mut d);
    logical_min(0, &mut d);
    logical_max(255, &mut d);
    report_size(8, &mut d);
    report_count(1, &mut d);
    input(INPUT_DVA, &mut d);
    assert_eq!(parse_report_desc(&d), Err(HidError::ReportId));
}

#[test]
fn buttons_only_is_not_stick() {
    let mut d = std::vec::Vec::new();
    page(PAGE_BUTTON, &mut d);
    usage_min(1, &mut d);
    usage_max(2, &mut d);
    logical_min(0, &mut d);
    logical_max(1, &mut d);
    report_size(1, &mut d);
    report_count(2, &mut d);
    input(INPUT_DVA, &mut d);
    assert_eq!(parse_report_desc(&d), Err(HidError::NotStick));
    assert_eq!(parse_report_desc(&[]), Err(HidError::NotStick));
}

#[test]
fn too_many_local_usages_rejected() {
    // 9 queued local Usages on one Main item: a loud parse error, not
    // a silent clobber of the queue tail (review finding on 246cfec).
    let mut d = std::vec::Vec::new();
    page(PAGE_GENERIC_DESKTOP, &mut d);
    for u in [
        GD_X, GD_Y, GD_Z, GD_RX, GD_RY, GD_RZ, GD_SLIDER, GD_HAT, GD_X,
    ] {
        usage(u, &mut d);
    }
    logical_min(0, &mut d);
    logical_max(255, &mut d);
    report_size(8, &mut d);
    report_count(9, &mut d);
    input(INPUT_DVA, &mut d);
    assert_eq!(parse_report_desc(&d), Err(HidError::TooManyFields));
}

#[test]
fn long_items_are_skipped() {
    // Vendor long item (§6.2.2.3): 0xFE, size 2, tag 0xA0, 2 data bytes.
    let mut d = vec![0xFE, 0x02, 0xA0, 0xAB, 0xCD];
    d.extend(joystick_desc());
    assert_eq!(parse_ok(&d).n_fields, 23);
}

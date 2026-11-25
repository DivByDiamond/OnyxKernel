//! Host tests for the HID session's descriptor fetch: the HID 1.11
//! §7.2.1 request form over a scripted `ControlBus` double, the
//! short-read parse, and the NotStick / malformed / bus-error paths.

use super::MAX_REPORT;
use super::diff::apply_report;
use super::fetch::{DESC_READ, fetch_session, fetch_table};
use crate::drivers::usb::dev::requests::{ControlBus, REQ_GET_DESCRIPTOR, RTYPE_IFACE_IN, setup};
use crate::drivers::usb::hid::report::fixtures::{
    INPUT_DVA, input, joystick_desc, logical_max, logical_min, page, report_count, report_size,
    usage_max, usage_min,
};
use crate::drivers::usb::hid::{FieldKind, HID_DESC_REPORT, HidEvent, MAX_FIELDS, PAGE_BUTTON};
use onyx_core::errno::{Errno, KResult};
use std::vec::Vec;

/// Interface the scripted descriptor read must target — nonzero to
/// prove wIndex carries the slot's interface number, not a hardcoded 0.
const IFACE: u8 = 2;

/// Scripted `ControlBus` double: serves exactly one IN control
/// transfer — the report-descriptor read — asserting the HID 1.11
/// §7.2.1 request form byte-exactly (bmRequestType 0x81,
/// GET_DESCRIPTOR, wValue = 0x22 << 8, wIndex = the interface, wLength
/// = the bounded read). `fail` scripts a raw bus error instead.
struct FakeBus {
    reply: Vec<u8>,
    fail: Option<Errno>,
}

impl ControlBus for FakeBus {
    fn control(&mut self, pkt: &[u8; 8], data: Option<&mut [u8]>, data_in: bool) -> KResult<u32> {
        if let Some(e) = self.fail {
            return Err(e);
        }
        assert!(data_in, "descriptor read must be IN");
        let value = u16::from(HID_DESC_REPORT) << 8;
        assert_eq!(
            pkt,
            &setup(
                RTYPE_IFACE_IN,
                REQ_GET_DESCRIPTOR,
                value,
                u16::from(IFACE),
                DESC_READ,
            ),
            "HID 1.11 §7.2.1 request form"
        );
        let buf = data.ok_or(Errno::Io)?;
        let n = self.reply.len().min(buf.len());
        buf[..n].copy_from_slice(&self.reply[..n]);
        Ok(n as u32)
    }

    fn set_addr(&mut self, _addr: u8) {}
}

fn fake(desc: &[u8]) -> FakeBus {
    FakeBus {
        reply: desc.to_vec(),
        fail: None,
    }
}

/// A button-only descriptor (no Generic Desktop axis): parseable, but
/// not a stick.
fn button_only_desc() -> Vec<u8> {
    let mut d = Vec::new();
    page(PAGE_BUTTON, &mut d);
    usage_min(1, &mut d);
    usage_max(4, &mut d);
    logical_min(0, &mut d);
    logical_max(1, &mut d);
    report_size(1, &mut d);
    report_count(4, &mut d);
    input(INPUT_DVA, &mut d);
    d
}

#[test]
fn fetch_parses_joystick_fixture_over_short_read() {
    // The fixture is ~40 bytes: the device answers the bounded 512-byte
    // read SHORT, and the returned length is what gets parsed.
    let desc = joystick_desc();
    let mut bus = fake(&desc);
    let t = fetch_table(&mut bus, IFACE).unwrap();
    assert_eq!(t.n_fields, 23);
    assert_eq!(t.fields[0].kind, FieldKind::Axis(0));
}

#[test]
fn fetch_button_only_descriptor_is_not_stick_noent() {
    // NotStick maps to NoEnt: the slot stays a plain enumerated device
    // (the usb-kbd smoke case) — the caller logs info, not a warning.
    let mut bus = fake(&button_only_desc());
    assert_eq!(fetch_table(&mut bus, IFACE), Err(Errno::NoEnt));
}

#[test]
fn fetch_truncated_descriptor_is_inval() {
    // A short read that cuts a 2-byte-data item (the ±32767 logical
    // minimum, 0x16) mid-payload: parse Short → Inval.
    let desc = joystick_desc();
    let pos = desc
        .iter()
        .position(|&b| b == 0x16)
        .expect("fixture has a 2-byte logical minimum");
    let cut = &desc[..pos + 2];
    let mut bus = fake(cut);
    assert_eq!(fetch_table(&mut bus, IFACE), Err(Errno::Inval));
}

#[test]
fn fetch_report_id_descriptor_is_inval() {
    // Report IDs are rejected outright (task 6, ruling 4).
    let mut bus = fake(&[0x85, 0x01]);
    assert_eq!(fetch_table(&mut bus, IFACE), Err(Errno::Inval));
}

#[test]
fn fetch_unknown_item_is_inval() {
    // Reserved MAIN item 0xD1 (tag 0xD0, 1 data byte): UnknownItem.
    let mut bus = fake(&[0xD1, 0x00]);
    assert_eq!(fetch_table(&mut bus, IFACE), Err(Errno::Inval));
}

#[test]
fn fetch_bus_error_and_empty_read_are_io() {
    let mut bus = fake(&joystick_desc());
    bus.fail = Some(Errno::Io);
    assert_eq!(fetch_table(&mut bus, IFACE), Err(Errno::Io));
    // A zero-length read carries no descriptor to parse.
    let mut bus = fake(&[]);
    assert_eq!(fetch_table(&mut bus, IFACE), Err(Errno::Io));
}

/// Bus for the whole attach read sequence: the report descriptor, then
/// GET_REPORT, answered with `input_report` (`None` = the device stalls).
struct AttachBus {
    input_report: Option<Vec<u8>>,
}

impl ControlBus for AttachBus {
    fn control(&mut self, pkt: &[u8; 8], data: Option<&mut [u8]>, _in: bool) -> KResult<u32> {
        let reply = if pkt[1] == REQ_GET_DESCRIPTOR {
            joystick_desc().to_vec()
        } else {
            // HID 1.11 §7.2.1: GET_REPORT, Input type, report id 0.
            let want = setup(0xA1, 0x01, 0x0100, u16::from(IFACE), MAX_REPORT as u16);
            assert_eq!(pkt, &want, "GET_REPORT request form");
            self.input_report.clone().ok_or(Errno::Io)?
        };
        let buf = data.ok_or(Errno::Io)?;
        let n = reply.len().min(buf.len());
        buf[..n].copy_from_slice(&reply[..n]);
        Ok(n as u32)
    }

    fn set_addr(&mut self, _addr: u8) {}
}

/// Attach over `bus`, then feed one report that moves only axis 0.
fn first_motion(mut bus: AttachBus) -> Vec<(HidEvent, bool)> {
    let mut session = fetch_session(&mut bus, IFACE).unwrap();
    let mut report = [0u8; 13];
    report[0..2].copy_from_slice(&1000i16.to_le_bytes());
    let mut out = [(HidEvent::Axis { axis: 0, value: 0 }, false); MAX_FIELDS];
    let n = apply_report(&mut session, &report, &mut out);
    out[..n].to_vec()
}

/// Regression: a device that reports only on change (QEMU usb-tablet)
/// had its first real motion delivered as the `init` burst.
#[test]
fn get_report_baseline_makes_the_first_motion_a_live_event() {
    let evs = first_motion(AttachBus {
        input_report: Some([0u8; 13].to_vec()),
    });
    let moved = HidEvent::Axis {
        axis: 0,
        value: 1000,
    };
    assert_eq!(evs, [(moved, false)]);
}

#[test]
fn stalled_or_short_get_report_keeps_the_first_report_as_init() {
    for input_report in [None, Some([0u8; 2].to_vec())] {
        let evs = first_motion(AttachBus { input_report });
        assert_eq!(evs.len(), 23);
        assert!(evs.iter().all(|(_, init)| *init));
    }
}

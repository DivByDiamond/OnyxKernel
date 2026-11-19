//! Byte-exact fixtures for the setup-packet builders, plus a scripted
//! `ControlBus` double and host-side coverage of the `LiveBus` contract.

use super::*;
use crate::drivers::usb::dev::desc::DESC_CONFIGURATION;
use crate::drivers::usb::dev::desc::DESC_DEVICE;
use onyx_core::errno::Errno;

#[test]
fn setup_lays_out_fields_little_endian() {
    // bmRequestType=0x21, bRequest=0x09, wValue=0x0301, wIndex=0x0002,
    // wLength=0x000a — all 16-bit fields serialized low byte first.
    let p = setup(0x21, 0x09, 0x0301, 0x0002, 0x000a);
    assert_eq!(p, [0x21, 0x09, 0x01, 0x03, 0x02, 0x00, 0x0a, 0x00]);
}

#[test]
fn get_descriptor_device_is_standard_dev_in() {
    let p = get_descriptor(DESC_DEVICE, 0, 64);
    assert_eq!(p, [0x80, 0x06, 0x01, 0x00, 0x00, 0x00, 0x40, 0x00]);
}

#[test]
fn get_descriptor_packs_type_and_index_into_wvalue() {
    // Config descriptor, index 0, first 9 bytes: byte 2 is the type,
    // byte 3 the index (the plan's byte order).
    let p = get_descriptor(DESC_CONFIGURATION, 0, 9);
    assert_eq!(p[0], 0x80);
    assert_eq!(p[1], REQ_GET_DESCRIPTOR);
    assert_eq!([p[2], p[3]], [DESC_CONFIGURATION, 0]);
}

#[test]
fn set_address_carries_addr_in_wvalue() {
    let p = set_address(5);
    assert_eq!(p, [0x00, 0x05, 0x05, 0x00, 0x00, 0x00, 0x00, 0x00]);
}

#[test]
fn set_configuration_carries_cfg_in_wvalue() {
    let p = set_configuration(1);
    assert_eq!(p, [0x00, 0x09, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00]);
}

/// Scripted double: records the last request so a trait consumer can be
/// tested without hardware (Task 3's FakeBus follows this shape).
struct FakeBus {
    last_setup: [u8; 8],
    last_in: bool,
}

impl ControlBus for FakeBus {
    fn control(&mut self, setup: &[u8; 8], data: Option<&mut [u8]>, data_in: bool) -> KResult<u32> {
        self.last_setup = *setup;
        self.last_in = data_in;
        match data {
            Some(d) if data_in => {
                d[0] = 0x12;
                Ok(d.len() as u32)
            }
            Some(d) => Ok(d.len() as u32),
            None => Ok(0),
        }
    }
}

#[test]
fn control_bus_double_records_request_and_fills_in_data() {
    let mut bus = FakeBus {
        last_setup: [0u8; 8],
        last_in: false,
    };
    let mut buf = [0u8; 2];
    let n = bus
        .control(&set_address(7), Some(&mut buf), true)
        .unwrap_or(0);
    assert_eq!(n, 2);
    assert_eq!(bus.last_setup, set_address(7));
    assert!(bus.last_in);
    assert_eq!(buf, [0x12, 0]);
}

#[test]
fn live_bus_without_active_controller_reports_nosys() {
    // SAFETY: no controller is initialized in the host test binary, so
    // the dispatch reads the inert G_ACTIVE (None) and takes the
    // memory-safe Err(NoSys) branch without touching hardware.
    let mut bus = unsafe { LiveBus::new(5) };
    let mut buf = [0u8; 8];
    assert_eq!(
        bus.control(&set_address(5), Some(&mut buf), true),
        Err(Errno::NoSys)
    );
}

#[test]
fn live_bus_set_addr_retargets_without_panic() {
    // SAFETY: as above — the host dispatch path is the inert None branch.
    let mut bus = unsafe { LiveBus::new(0) };
    bus.set_addr(42);
    assert_eq!(
        bus.control(&set_configuration(1), None, false),
        Err(Errno::NoSys)
    );
}

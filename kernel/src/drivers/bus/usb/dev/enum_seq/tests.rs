//! Addressing rule of the enumeration pass: SET_ADDRESS is sent only on
//! buses where the driver owns the device address.

use super::enumerate;
use crate::drivers::usb::dev::Owner;
use crate::drivers::usb::dev::desc::tests::{config_fixture, device_fixture};
use crate::drivers::usb::dev::desc::{DESC_CONFIGURATION, DESC_DEVICE};
use crate::drivers::usb::dev::requests::{ControlBus, REQ_SET_ADDRESS};
use onyx_core::errno::{Errno, KResult};
use std::vec::Vec;

/// Bus of a controller that addresses devices itself (xHCI): serves the
/// descriptors and records every request code it sees.
struct ControllerAddressedBus {
    requests: Vec<u8>,
}

impl ControlBus for ControllerAddressedBus {
    fn control(&mut self, pkt: &[u8; 8], data: Option<&mut [u8]>, _in: bool) -> KResult<u32> {
        self.requests.push(pkt[1]);
        let Some(buf) = data else {
            return Ok(0);
        };
        let reply = match pkt[3] {
            DESC_DEVICE => device_fixture().to_vec(),
            DESC_CONFIGURATION => config_fixture().to_vec(),
            _ => return Err(Errno::Io),
        };
        let n = reply.len().min(buf.len());
        buf[..n].copy_from_slice(&reply[..n]);
        Ok(n as u32)
    }

    fn set_addr(&mut self, _addr: u8) {
        panic!("a controller-addressed bus is never re-targeted");
    }

    fn assigns_address(&self) -> bool {
        true
    }
}

/// Regression: SET_ADDRESS(1) was sent to every xHCI device (forbidden by
/// the xHCI spec). QEMU then rang slot 1 for every device's wakeup, so a
/// second stick never delivered live input.
#[test]
fn controller_addressed_bus_gets_no_set_address() {
    let mut bus = ControllerAddressedBus {
        requests: Vec::new(),
    };
    let slot = enumerate(&mut bus, Owner::Xhci).unwrap();
    assert!(!bus.requests.contains(&REQ_SET_ADDRESS));
    assert_eq!(slot.addr, 0, "the driver does not know the address");
}

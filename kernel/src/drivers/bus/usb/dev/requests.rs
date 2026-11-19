//! USB standard control-request builders (8-byte setup packets) and the
//! `ControlBus` abstraction over the control-transfer path.

use onyx_core::errno::KResult;

/// bRequest: SET_ADDRESS.
pub const REQ_SET_ADDRESS: u8 = 0x05;
/// bRequest: GET_DESCRIPTOR.
pub const REQ_GET_DESCRIPTOR: u8 = 0x06;
/// bRequest: SET_CONFIGURATION.
pub const REQ_SET_CONFIGURATION: u8 = 0x09;

/// bmRequestType: host-to-device, standard, device recipient.
const RTYPE_DEV_OUT: u8 = 0x00;
/// bmRequestType: device-to-host, standard, device recipient.
const RTYPE_DEV_IN: u8 = 0x80;

/// Build an 8-byte USB setup packet: `rtype`/`request` as-is, the
/// 16-bit `value`/`index`/`len` fields little-endian.
pub fn setup(rtype: u8, request: u8, value: u16, index: u16, len: u16) -> [u8; 8] {
    let v = value.to_le_bytes();
    let i = index.to_le_bytes();
    let l = len.to_le_bytes();
    [rtype, request, v[0], v[1], i[0], i[1], l[0], l[1]]
}

/// GET_DESCRIPTOR setup packet for descriptor type `typ` at `index`,
/// asking for up to `len` bytes back: wire bytes 2..4 carry the type,
/// then the index (the byte order of the plan's published patterns).
pub fn get_descriptor(typ: u8, index: u8, len: u16) -> [u8; 8] {
    let value = ((index as u16) << 8) | typ as u16;
    setup(RTYPE_DEV_IN, REQ_GET_DESCRIPTOR, value, 0, len)
}

/// SET_ADDRESS setup packet assigning the device address `addr`.
pub fn set_address(addr: u8) -> [u8; 8] {
    setup(RTYPE_DEV_OUT, REQ_SET_ADDRESS, addr as u16, 0, 0)
}

/// SET_CONFIGURATION setup packet selecting configuration `cfg`.
pub fn set_configuration(cfg: u8) -> [u8; 8] {
    setup(RTYPE_DEV_OUT, REQ_SET_CONFIGURATION, cfg as u16, 0, 0)
}

/// Testable abstraction over the control-transfer path of one USB
/// controller.
///
/// The bus is *addressed*: an implementation carries the device address
/// of its target (fixed at construction, re-targetable via an inherent
/// setter for the re-address step after SET_ADDRESS), while the setup
/// packet carries the request itself.
pub trait ControlBus {
    /// Issue one control transfer: send `setup`, then move up to
    /// `data.len()` bytes in the direction `data_in` (`true` = IN,
    /// device → host). Returns the number of data bytes moved.
    fn control(&mut self, setup: &[u8; 8], data: Option<&mut [u8]>, data_in: bool) -> KResult<u32>;
}

/// Live bus wrapping `crate::drivers::usb::control_transfer`; the caller
/// holds the USB lock.
pub struct LiveBus {
    dev_addr: u8,
}

impl LiveBus {
    /// # Safety
    ///
    /// Requires a prior successful `init_usb` (an active controller
    /// recorded in the dispatch state), mirroring the contract of
    /// `usb::control_transfer`. Construct one per enumerated device
    /// address.
    pub unsafe fn new(dev_addr: u8) -> Self {
        LiveBus { dev_addr }
    }

    /// Re-target the bus after the SET_ADDRESS step completes.
    pub fn set_addr(&mut self, dev_addr: u8) {
        self.dev_addr = dev_addr;
    }
}

impl ControlBus for LiveBus {
    fn control(&mut self, setup: &[u8; 8], data: Option<&mut [u8]>, data_in: bool) -> KResult<u32> {
        // Mirror the hcd.rs URB wrappers: max_pkt is the data length,
        // clamped to at least 8 so a no-data phase stays control-sized.
        let max_pkt = data.as_deref().map_or(0, |d| d.len() as u32).max(8);
        // SAFETY: the constructor's contract requires a prior successful init_usb, and the caller holds the USB lock for the whole transfer.
        unsafe {
            crate::drivers::usb::control_transfer(self.dev_addr, setup, data, data_in, max_pkt)
        }
    }
}

#[cfg(test)]
mod tests;

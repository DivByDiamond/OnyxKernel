//! USB standard control-request builders (8-byte setup packets) and the
//! `ControlBus` abstraction over the control-transfer path.

use onyx_core::errno::KResult;

use super::Owner;

/// bRequest: SET_ADDRESS.
pub const REQ_SET_ADDRESS: u8 = 0x05;
/// bRequest: GET_DESCRIPTOR.
pub const REQ_GET_DESCRIPTOR: u8 = 0x06;
/// bRequest: SET_CONFIGURATION.
pub const REQ_SET_CONFIGURATION: u8 = 0x09;

/// The address every USB device answers to straight out of reset
/// (USB 2.0 §9.1.2): enumeration constructs the bus here and re-targets
/// it with [`ControlBus::set_addr`] after SET_ADDRESS.
pub const ADDR_DEFAULT: u8 = 0;

/// bmRequestType: host-to-device, standard, device recipient.
const RTYPE_DEV_OUT: u8 = 0x00;
/// bmRequestType: device-to-host, standard, device recipient.
const RTYPE_DEV_IN: u8 = 0x80;
/// bmRequestType: device-to-host, standard, INTERFACE recipient (the
/// HID 1.11 §7.2.1 report-descriptor read rides this form).
pub const RTYPE_IFACE_IN: u8 = 0x81;

/// Build an 8-byte USB setup packet: `rtype`/`request` as-is, the
/// 16-bit `value`/`index`/`len` fields little-endian.
pub fn setup(rtype: u8, request: u8, value: u16, index: u16, len: u16) -> [u8; 8] {
    let v = value.to_le_bytes();
    let i = index.to_le_bytes();
    let l = len.to_le_bytes();
    [rtype, request, v[0], v[1], i[0], i[1], l[0], l[1]]
}

/// GET_DESCRIPTOR setup packet for descriptor type `typ` at `index`,
/// asking for up to `len` bytes back. Per USB 2.0 §9.4.3, wValue packs
/// the descriptor type in its high byte and the index in its low byte,
/// so wire byte 2 is the index and byte 3 the type.
pub fn get_descriptor(typ: u8, index: u8, len: u16) -> [u8; 8] {
    let value = (u16::from(typ) << 8) | u16::from(index);
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
/// of its target (fixed at construction, re-targetable for the
/// re-address step after SET_ADDRESS), while the setup packet carries
/// the request itself.
pub trait ControlBus {
    /// Issue one control transfer: send `setup`, then move up to
    /// `data.len()` bytes in the direction `data_in` (`true` = IN,
    /// device → host). Returns the number of data bytes moved.
    fn control(&mut self, setup: &[u8; 8], data: Option<&mut [u8]>, data_in: bool) -> KResult<u32>;

    /// Re-target the bus after the enumeration SET_ADDRESS step
    /// completes: every later transfer is issued to `addr` instead of
    /// the address the bus was constructed with.
    fn set_addr(&mut self, addr: u8);

    /// Controller-side handle of the target device (the xHCI slot id),
    /// recorded in the registry slot at enumeration. Buses over
    /// controllers that address by USB address report 0.
    fn hc_slot(&self) -> u8 {
        0
    }
}

/// Live bus routing one enumerated device's control transfers to its
/// OWNING controller (ruling 2): with the dual SG2000 init (EHCI +
/// OHCI companion) the single-active-controller dispatch is ambiguous,
/// so the bus carries its owner from construction and `control` goes to
/// that controller's engine directly — the legacy `usb::control_transfer`
/// + `G_ACTIVE` path stays for its existing callers (`hcd.rs`).
pub struct LiveBus {
    owner: Owner,
    dev_addr: u8,
    /// xHCI slot id of the target (0 for the other controllers).
    hc_slot: u8,
}

impl LiveBus {
    /// # Safety
    ///
    /// Requires `owner`'s controller to have been initialized
    /// (`ehci::init_ehci`/`ohci::init_ohci`/`xhci::init`): `control`
    /// routes to that controller directly. The caller holds the USB
    /// lock for the whole transfer. Construct one per enumerated device
    /// address.
    pub unsafe fn new(owner: Owner, dev_addr: u8) -> Self {
        LiveBus {
            owner,
            dev_addr,
            hc_slot: 0,
        }
    }

    /// Target the xHCI device of controller slot `hc_slot`: xHCI
    /// transfers are routed by slot id, not by USB address.
    pub fn with_hc_slot(mut self, hc_slot: u8) -> Self {
        self.hc_slot = hc_slot;
        self
    }
}

impl ControlBus for LiveBus {
    fn control(&mut self, setup: &[u8; 8], data: Option<&mut [u8]>, data_in: bool) -> KResult<u32> {
        // Mirror the hcd.rs URB wrappers: max_pkt is the data length,
        // clamped to at least 8 so a no-data phase stays control-sized.
        let max_pkt = data.as_deref().map_or(0, |d| d.len() as u32).max(8);
        // SAFETY: the constructor's contract requires the owner's
        // controller initialized, and the caller holds the USB lock for
        // the whole transfer.
        unsafe {
            match self.owner {
                Owner::Xhci => crate::drivers::usb::xhci::xhci_control_transfer(
                    self.hc_slot,
                    setup,
                    data,
                    data_in,
                    max_pkt,
                ),
                Owner::Ehci => crate::drivers::usb::ehci::ehci_control_transfer(
                    self.dev_addr,
                    setup,
                    data,
                    data_in,
                    max_pkt,
                ),
                // Full speed: the registry carries no speed bit, and the
                // SG2000 companion path targets full-speed HID.
                Owner::Ohci => crate::drivers::usb::ohci::ohci_control_transfer(
                    self.dev_addr,
                    setup,
                    data,
                    data_in,
                    max_pkt,
                    0,
                ),
            }
        }
    }

    fn set_addr(&mut self, dev_addr: u8) {
        self.dev_addr = dev_addr;
    }

    fn hc_slot(&self) -> u8 {
        self.hc_slot
    }
}

#[cfg(test)]
mod tests;

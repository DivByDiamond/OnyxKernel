pub mod core;
pub mod ehci;
pub mod hcd;
pub mod ohci;
pub mod xhci;

use onyx_core::errno::{Errno, KResult};

#[derive(PartialEq, Clone, Copy)]
enum ControllerType {
    None,
    Ehci,
    Ohci,
    Xhci,
}
static mut G_ACTIVE: ControllerType = ControllerType::None;

pub const EHCI_BASE: usize = 0x04C0_0000;
pub const OHCI_BASE: usize = 0x04C1_0000;

/// # Safety
///
/// Requires a prior successful `init_usb` (a controller probed, initialized,
/// and recorded in `G_ACTIVE`); run with SIE=0 per `crate::sync`.
pub unsafe fn control_transfer(
    dev_addr: u8,
    setup_pkt: &[u8; 8],
    data: Option<&mut [u8]>,
    data_in: bool,
    max_pkt: u32,
) -> KResult<u32> {
    // SAFETY: G_ACTIVE is written once by init_usb during single-threaded boot init; here it is only read to dispatch to the active controller.
    unsafe {
        match G_ACTIVE {
            ControllerType::Ehci => {
                ehci::ehci_control_transfer(dev_addr, setup_pkt, data, data_in, max_pkt)
            }
            ControllerType::Ohci => {
                ohci::ohci_control_transfer(dev_addr, setup_pkt, data, data_in, max_pkt, 0)
            }
            ControllerType::Xhci => {
                xhci::xhci_control_transfer(dev_addr, setup_pkt, data, data_in, max_pkt)
            }
            ControllerType::None => Err(Errno::NoSys),
        }
    }
}

/// Direction bit of a USB endpoint address: bit 7 set means an IN endpoint
/// (device → host). Shared by the bulk dispatch paths.
fn endpoint_is_in(endpoint: u8) -> bool {
    (endpoint & 0x80) != 0
}

/// # Safety
///
/// Requires a prior successful `init_usb`; `dev_addr`/`endpoint` must name a
/// device the active controller has already configured, and `data` must stay
/// valid and DMA-addressable for the duration of the transfer.
pub unsafe fn bulk_transfer(dev_addr: u8, endpoint: u8, data: Option<&mut [u8]>) -> KResult<u32> {
    let data_in = endpoint_is_in(endpoint);
    // SAFETY: G_ACTIVE is written once by init_usb during single-threaded boot init; here it is only read to dispatch to the active controller.
    unsafe {
        match G_ACTIVE {
            ControllerType::Ehci => ehci::ehci_bulk_transfer(dev_addr, data, data_in, 512),
            ControllerType::Ohci => ohci::ohci_bulk_transfer(dev_addr, data, data_in, 512, 0),
            ControllerType::Xhci => xhci::xhci_bulk_transfer(dev_addr, endpoint, data),
            ControllerType::None => Err(Errno::NoSys),
        }
    }
}

pub fn n_ports() -> u8 {
    // SAFETY: plain read of G_ACTIVE, written once during single-threaded USB init; SIE=0 in kernel context (see crate::sync).
    unsafe {
        match G_ACTIVE {
            ControllerType::Ehci => ehci::ehci_n_ports(),
            ControllerType::Ohci => ohci::ohci_n_ports(),
            ControllerType::Xhci => xhci::xhci_n_ports(),
            ControllerType::None => 0,
        }
    }
}

/// # Safety
///
/// Requires a prior successful `init_usb`; `idx` is forwarded to the active
/// controller (bounds checked there).
pub unsafe fn port_status(idx: u8) -> KResult<u32> {
    // SAFETY: G_ACTIVE is written once by init_usb during single-threaded boot init; here it is only read to dispatch to the active controller.
    unsafe {
        match G_ACTIVE {
            ControllerType::Ehci => ehci::ehci_port_status(idx),
            ControllerType::Ohci => ohci::ohci_port_status(idx),
            ControllerType::Xhci => xhci::xhci_port_status(idx),
            ControllerType::None => Err(Errno::NoSys),
        }
    }
}

/// # Safety
///
/// Requires a prior successful `init_usb`; `idx` is forwarded to the active
/// controller (bounds checked there).
pub unsafe fn port_reset(idx: u8) -> KResult<()> {
    // SAFETY: G_ACTIVE is written once by init_usb during single-threaded boot init; here it is only read to dispatch to the active controller.
    unsafe {
        match G_ACTIVE {
            ControllerType::Ehci => ehci::ehci_port_reset(idx),
            ControllerType::Ohci => ohci::ohci_port_reset(idx),
            ControllerType::Xhci => xhci::xhci_port_reset(idx),
            ControllerType::None => Err(Errno::NoSys),
        }
    }
}

/// # Safety
///
/// Requires a prior successful `init_usb`; `idx` is forwarded to the active
/// controller (bounds checked there).
pub unsafe fn port_enable(idx: u8) -> KResult<()> {
    // SAFETY: G_ACTIVE is written once by init_usb during single-threaded boot init; here it is only read to dispatch to the active controller.
    unsafe {
        match G_ACTIVE {
            ControllerType::Ehci => ehci::ehci_port_enable(idx),
            ControllerType::Ohci => ohci::ohci_port_enable(idx),
            ControllerType::Xhci => xhci::xhci_port_enable(idx),
            ControllerType::None => Err(Errno::NoSys),
        }
    }
}

/// # Safety
///
/// Must run once on the boot hart during single-threaded early boot before
/// secondary harts start; probes the hardcoded SG2000 controller MMIO bases
/// (`EHCI_BASE`/`OHCI_BASE`), identity-mapped on that target only.
pub unsafe fn init_usb() -> KResult<()> {
    // SAFETY: EHCI_BASE/OHCI_BASE are the SG2000 platform controller bases, identity-mapped at boot; probe_* validates each before init touches registers.
    unsafe {
        if ehci::probe_ehci(EHCI_BASE) {
            ehci::init_ehci(EHCI_BASE)
        } else if ohci::probe_ohci(OHCI_BASE) {
            ohci::init_ohci(OHCI_BASE)
        } else {
            Err(Errno::NoEnt)
        }
    }
}

#[cfg(test)]
mod tests;

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
}
static mut G_ACTIVE: ControllerType = ControllerType::None;

pub const EHCI_BASE: usize = 0x04C0_0000;
pub const OHCI_BASE: usize = 0x04C1_0000;

pub unsafe fn control_transfer(
    dev_addr: u8,
    setup_pkt: &[u8; 8],
    data: Option<&mut [u8]>,
    data_in: bool,
    max_pkt: u32,
) -> KResult<u32> {
    match G_ACTIVE {
        ControllerType::Ehci => ehci::ehci_control_transfer(dev_addr, setup_pkt, data, data_in, max_pkt),
        ControllerType::Ohci => {
            ohci::ohci_control_transfer(dev_addr, setup_pkt, data, data_in, max_pkt, 0)
        }
        ControllerType::None => Err(Errno::NoSys),
    }
}

pub fn n_ports() -> u8 {
    unsafe {
        match G_ACTIVE {
            ControllerType::Ehci => ehci::ehci_n_ports(),
            ControllerType::Ohci => ohci::ohci_n_ports(),
            ControllerType::None => 0,
        }
    }
}

pub unsafe fn port_status(idx: u8) -> KResult<u32> {
    match G_ACTIVE {
        ControllerType::Ehci => ehci::ehci_port_status(idx),
        ControllerType::Ohci => ohci::ohci_port_status(idx),
        ControllerType::None => Err(Errno::NoSys),
    }
}

pub unsafe fn port_reset(idx: u8) -> KResult<()> {
    match G_ACTIVE {
        ControllerType::Ehci => ehci::ehci_port_reset(idx),
        ControllerType::Ohci => ohci::ohci_port_reset(idx),
        ControllerType::None => Err(Errno::NoSys),
    }
}

pub unsafe fn port_enable(idx: u8) -> KResult<()> {
    match G_ACTIVE {
        ControllerType::Ehci => ehci::ehci_port_enable(idx),
        ControllerType::Ohci => ohci::ohci_port_enable(idx),
        ControllerType::None => Err(Errno::NoSys),
    }
}

pub unsafe fn init_usb() -> KResult<()> {
    if ehci::probe_ehci(EHCI_BASE) {
        ehci::init_ehci(EHCI_BASE)
    } else if ohci::probe_ohci(OHCI_BASE) {
        ohci::init_ohci(OHCI_BASE)
    } else {
        Err(Errno::NoEnt)
    }
}

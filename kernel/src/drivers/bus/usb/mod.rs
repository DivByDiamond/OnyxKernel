pub mod core;
pub mod dev;
pub mod ehci;
pub mod hcd;
pub mod hid;
pub mod ohci;
pub mod ports;
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
/// Requires a prior successful `init_usb` (a controller probed,
/// initialized, and recorded in `G_ACTIVE`); run with SIE=0 per
/// `crate::sync`.
///
/// Legacy single-controller dispatch (ruling 2): `G_ACTIVE` names the
/// LAST controller that initialized — with the SG2000 dual init that is
/// the OHCI companion, which owns the full/low-speed devices the
/// legacy URB path serves. Owner-correct routing across both
/// controllers goes through the `owner_*` dispatchers and
/// `dev::requests::LiveBus`.
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

/// # Safety
///
/// Requires a prior successful `init_usb`; `slot_idx` must name an active
/// registry slot whose owner decides the controller path (inactive or
/// out-of-range slots are `Errno::NoEnt`). Runs in the single-consumer USB
/// poll context with SIE=0 (see `crate::sync`), serialized with every other
/// USB transfer path; the registry lock is taken internally, so the
/// caller must not hold `USB_DEV_LOCK`.
///
/// `buf` must hold at least the slot's interrupt-IN max packet size and
/// stay valid for the duration of the poll. THE POLL IS NON-BLOCKING
/// on BOTH controllers (ruling 2 — the 100 Hz input-poll loop spins on
/// it): `Ok(0)` = no report ready, `Ok(n)` = one report of n bytes,
/// `Err(Errno::Io)` = the transfer error-retired (the controller path
/// re-arms itself, so polling may recover), `Err(NoEnt)` = the channel
/// was never armed. EHCI interrupt transfers stay out of scope
/// (ruling 1): an Ehci-owned slot reports `Errno::NoSys` here — the
/// arm-time failure is kwrn-logged by the boot probe, so this silent
/// branch cannot spam a poll loop.
pub unsafe fn interrupt_transfer(slot_idx: usize, buf: &mut [u8]) -> KResult<u32> {
    // SAFETY: the registry read is lock-serialized inside `dev::slot`;
    // the dispatch targets are the controllers' single-owner paths.
    unsafe {
        match dev::slot(slot_idx) {
            Some(s) => match s.owner {
                dev::Owner::Xhci => xhci::xhci_poll_interrupt(buf),
                dev::Owner::Ohci => ohci::ohci_poll_interrupt(buf),
                dev::Owner::Ehci => Err(Errno::NoSys),
            },
            None => Err(Errno::NoEnt),
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

/// Dual-controller port dispatch lives in [`ports`] (ruling 1): the
/// boot probe routes one named owner's controller through it — the
/// `G_ACTIVE` legacy dispatchers above follow only the last
/// controller that initialized.
///
/// # Safety
///
/// Must run once on the boot hart during single-threaded early boot
/// before secondary harts start; probes the hardcoded SG2000 controller
/// MMIO bases (`EHCI_BASE`/`OHCI_BASE`), identity-mapped on that target
/// only. Dual init (ruling 1): BOTH controllers when both probe OK —
/// EHCI (high-speed) first, OHCI as the full/low-speed companion — or
/// whichever single one is found. `Err(Errno::NoEnt)` when neither
/// comes up; a partial failure (one controller dead) still succeeds.
pub unsafe fn init_usb() -> KResult<()> {
    // SAFETY: EHCI_BASE/OHCI_BASE are the SG2000 platform controller
    // bases, identity-mapped at boot; each init re-validates its own
    // base (probe_*) before touching registers.
    unsafe {
        let mut any = false;
        if ehci::probe_ehci(EHCI_BASE) && ehci::init_ehci(EHCI_BASE).is_ok() {
            any = true;
        }
        if ohci::probe_ohci(OHCI_BASE) && ohci::init_ohci(OHCI_BASE).is_ok() {
            any = true;
        }
        if !any {
            return Err(Errno::NoEnt);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;

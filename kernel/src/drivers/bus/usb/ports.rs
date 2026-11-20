//! Dual-controller port dispatch (ruling 1): the SG2000 boot probe
//! scans EHCI (high-speed) ports first, then the OHCI full/low-speed
//! companion's — these helpers route one NAMED owner's controller,
//! unlike the `G_ACTIVE` legacy dispatchers in `mod.rs`, which follow
//! whichever controller initialized last.

use super::dev;
use onyx_core::errno::KResult;

/// Port count of `owner`'s controller (0 when that controller never
/// initialized — a plain static read per branch).
pub fn owner_n_ports(owner: dev::Owner) -> u8 {
    // SAFETY: plain static reads written once during single-threaded
    // USB init; the xHCI branch also guards its operational flag.
    match owner {
        dev::Owner::Ehci => super::ehci::ehci_n_ports(),
        dev::Owner::Ohci => super::ohci::ohci_n_ports(),
        dev::Owner::Xhci => super::xhci::xhci_n_ports(),
    }
}

/// # Safety
///
/// Requires `owner`'s controller to have been initialized by
/// `init_usb`; `idx` is bounds-checked inside the target before any
/// MMIO.
pub unsafe fn owner_port_status(owner: dev::Owner, idx: u8) -> KResult<u32> {
    // SAFETY: per-owner targets re-check their own port bounds.
    unsafe {
        match owner {
            dev::Owner::Ehci => super::ehci::ehci_port_status(idx),
            dev::Owner::Ohci => super::ohci::ohci_port_status(idx),
            dev::Owner::Xhci => super::xhci::xhci_port_status(idx),
        }
    }
}

/// # Safety
///
/// Requires `owner`'s controller to have been initialized by
/// `init_usb`; `idx` is bounds-checked inside the target before any
/// MMIO.
pub unsafe fn owner_port_reset(owner: dev::Owner, idx: u8) -> KResult<()> {
    // SAFETY: per-owner targets re-check their own port bounds.
    unsafe {
        match owner {
            dev::Owner::Ehci => super::ehci::ehci_port_reset(idx),
            dev::Owner::Ohci => super::ohci::ohci_port_reset(idx),
            dev::Owner::Xhci => super::xhci::xhci_port_reset(idx),
        }
    }
}

/// # Safety
///
/// Requires `owner`'s controller to have been initialized by
/// `init_usb`; `idx` is bounds-checked inside the target before any
/// MMIO.
pub unsafe fn owner_port_enable(owner: dev::Owner, idx: u8) -> KResult<()> {
    // SAFETY: per-owner targets re-check their own port bounds.
    unsafe {
        match owner {
            dev::Owner::Ehci => super::ehci::ehci_port_enable(idx),
            dev::Owner::Ohci => super::ohci::ohci_port_enable(idx),
            dev::Owner::Xhci => super::xhci::xhci_port_enable(idx),
        }
    }
}

//! xHCI root-hub port access: connect status, reset and enable checks.

use onyx_core::errno::{Errno, KResult};

use super::{G_XHCI, regs};

/// # Safety
///
/// The driver must have been initialized via `init`; G_XHCI.obase must be a
/// valid operational register base. `port` must be < max_ports.
pub unsafe fn port_connect(port: u8) -> bool {
    // SAFETY: G_XHCI.obase was set by init from the validated controller
    // base; OP_PORTSC + port*0x10 is within the operational register file.
    unsafe {
        let reg = regs::OP_PORTSC + (port as u32) * 0x10;
        let v = regs::op_r32(G_XHCI.obase, reg);
        (v & regs::PORT_CCS) != 0
    }
}

/// Root-hub port count (0 until `init` runs).
pub(in crate::drivers::bus::usb) fn xhci_n_ports() -> u8 {
    // SAFETY: plain read of G_XHCI, written once by single-threaded init.
    unsafe {
        if G_XHCI.operational {
            G_XHCI.max_ports
        } else {
            0
        }
    }
}

/// # Safety
///
/// Requires `init` to have run (obase valid); `idx` is 0-based and
/// bounds-checked against `max_ports` below (PORTSC stride is 0x10).
pub(in crate::drivers::bus::usb) unsafe fn xhci_port_status(idx: u8) -> KResult<u32> {
    // SAFETY: obase from init; the bound check keeps the PORTSC offset
    // within the operational register file.
    unsafe {
        if !G_XHCI.operational || idx >= G_XHCI.max_ports {
            return Err(Errno::Range);
        }
        Ok(regs::op_r32(
            G_XHCI.obase,
            regs::OP_PORTSC + (idx as u32) * 0x10,
        ))
    }
}

/// # Safety
///
/// Same contract as [`xhci_port_status`].
pub(in crate::drivers::bus::usb) unsafe fn xhci_port_reset(idx: u8) -> KResult<()> {
    // SAFETY: same as xhci_port_status — validated obase, checked offset.
    unsafe {
        if !G_XHCI.operational || idx >= G_XHCI.max_ports {
            return Err(Errno::Range);
        }
        let reg = regs::OP_PORTSC + (idx as u32) * 0x10;
        // Set PR; the read-back write also acknowledges any pending change
        // (those bits are RW1C). Poll until the controller clears PR.
        let v = regs::op_r32(G_XHCI.obase, reg);
        regs::op_w32(G_XHCI.obase, reg, v | regs::PORT_PR);
        let mut timeout = 100_000u32;
        while timeout > 0 && (regs::op_r32(G_XHCI.obase, reg) & regs::PORT_PR) != 0 {
            timeout -= 1;
        }
        if timeout == 0 {
            return Err(Errno::Io);
        }
        Ok(())
    }
}

/// # Safety
///
/// Same contract as [`xhci_port_status`].
pub(in crate::drivers::bus::usb) unsafe fn xhci_port_enable(idx: u8) -> KResult<()> {
    // SAFETY: same as xhci_port_status — validated obase, checked offset.
    unsafe {
        if !G_XHCI.operational || idx >= G_XHCI.max_ports {
            return Err(Errno::Range);
        }
        // xHCI has no software port-enable: the controller enables the port
        // itself when a reset completes (PED bit) — verify that it did.
        let v = regs::op_r32(G_XHCI.obase, regs::OP_PORTSC + (idx as u32) * 0x10);
        if v & regs::PORT_PED != 0 {
            Ok(())
        } else {
            Err(Errno::Io)
        }
    }
}

//! xHCI discovery on the PCIe bus: class filter and BAR0 placement.
//!
//! QEMU `virt` exposes xHCI only as `-device qemu-xhci` (there is no
//! `generic-xhci` platform node) and ships no PCI firmware: BAR registers
//! read as 0, so the OS sizes and places them before the first MMIO access.
use onyx_core::fmt::Arg;

use super::{cfg_read, cfg_write, count, get, scan};

/// QEMU virt 32-bit PCIe MMIO window (`pcie@30000000` ranges in the DTB).
const MMIO_WINDOW_BASE: u64 = 0x4000_0000;

/// xHCI: class 0x0C (serial bus) / subclass 0x03 (USB) / prog-if 0x30.
/// `class` holds `(class << 8) | subclass`, so xHCI matches 0x0C03.
fn is_xhci(class: u16, prog_if: u8) -> bool {
    class == 0x0C03 && prog_if == 0x30
}

/// Size and place memory BAR0 inside [`MMIO_WINDOW_BASE`], preserving the
/// BAR's type bits; returns the assigned base.
///
/// # Safety
///
/// Same contract as [`find_xhci`]: single-threaded ECAM-backed bus, and the
/// caller owns the function (its BAR is not in use yet).
unsafe fn assign_bar0(bus: u8, dev: u8, func: u8) -> Option<u64> {
    // SAFETY: standard BAR offsets of a function the ECAM scan enumerated;
    // the probe runs before memory decode is enabled, so no MMIO is live.
    unsafe {
        let orig = cfg_read(bus, dev, func, 0x10);
        if orig & 1 != 0 {
            return None; // I/O BAR: xHCI uses memory space
        }
        cfg_write(bus, dev, func, 0x10, 0xFFFF_FFFF);
        let mask = cfg_read(bus, dev, func, 0x10);
        cfg_write(bus, dev, func, 0x10, orig);
        if mask == 0 || mask == 0xFFFF_FFFF {
            return None; // no implemented BAR
        }
        let size = (mask & 0xFFFF_FFF0).wrapping_neg() as u64;
        if !size.is_power_of_two() {
            return None;
        }
        let base = MMIO_WINDOW_BASE.next_multiple_of(size);
        if orig & 0x6 == 0x4 {
            // 64-bit BAR: the upper half stays 0 (the window is below 4 GiB).
            cfg_write(bus, dev, func, 0x14, 0);
        }
        cfg_write(bus, dev, func, 0x10, (base as u32) | (orig & 0xF));
        // Read back: a BAR that did not take the address would decode nowhere.
        let placed = (cfg_read(bus, dev, func, 0x10) & 0xFFFF_FFF0) as u64;
        (placed == base).then_some(base)
    }
}

/// Find the xHCI PCI function, size/place its BAR0 and enable its decoding.
///
/// # Safety
///
/// Single-threaded boot, before secondary harts start and before another
/// driver claims the controller. `ECAM_BASE` must be a real host bridge:
/// OC2R/sedna answers garbage (see `pci.rs`), so not there.
pub unsafe fn find_xhci() -> Option<u64> {
    // SAFETY: ECAM contract as for scan(); G_DEVS is written only by that
    // single-threaded scan and read back through the bounds-checked `get`;
    // the config accesses are standard offsets of an enumerated function.
    unsafe {
        if count() == 0 {
            scan();
        }
        for idx in 0..count() {
            let Ok(dev) = get(idx) else { continue };
            if !is_xhci(dev.class, dev.prog_if) {
                continue;
            }
            let bar0 = if dev.bar0 != 0 {
                dev.bar0
            } else {
                let Some(placed) = assign_bar0(dev.bus, dev.dev, dev.func) else {
                    continue;
                };
                placed
            };
            // Memory Space (bit 1) + Bus Master (bit 2): without them the BAR
            // stays undecoded and MMIO reads come back as all-ones.
            let cmd = cfg_read(dev.bus, dev.dev, dev.func, 0x04);
            cfg_write(dev.bus, dev.dev, dev.func, 0x04, cmd | 0x6);
            let enabled = cfg_read(dev.bus, dev.dev, dev.func, 0x04);
            crate::kinf!(
                "pci",
                "xhci %x:%x.%x bar0=%p cmd=%x->%x",
                Arg::from(dev.bus as u32),
                Arg::from(dev.dev as u32),
                Arg::from(dev.func as u32),
                Arg::from(bar0),
                Arg::from(cmd),
                Arg::from(enabled)
            );
            return (enabled & 0x6 == 0x6).then_some(bar0);
        }
        None
    }
}

#[cfg(test)]
mod tests;

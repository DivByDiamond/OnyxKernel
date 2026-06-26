//! USB host controller — EHCI + OHCI minimal register-level accessors.
//!
//! Both QEMU virt (xHCI but backward-compatible with EHCI at the legacy
//! I/O space) and the Milk-V Duo S (EHCI+OHCI pair) expose the same
//! register interface. This driver provides probe/init + a register
//! accessor; full URB submission is left to a future USB stack.
use crate::arch::mmio::Mmio;
use onyx_core::errno::{Errno, KResult};

pub const EHCI_BASE: usize = 0x04C0_0000;
pub const OHCI_BASE: usize = 0x04C1_0000;

// EHCI register offsets (capability + operational).
const EHCI_CAP_LENGTH: u32 = 0x00;
const EHCI_CAP_VERSION: u32 = 0x02;
const EHCI_CAP_HCSPARAMS: u32 = 0x04;
const EHCI_OP_USBCMD: u32 = 0x20;
const _EHCI_OP_USBSTS: u32 = 0x24;
const _EHCI_OP_USBINTR: u32 = 0x28;
const EHCI_OP_CONFIGFLAG: u32 = 0x40;
const EHCI_OP_PORTSC: u32 = 0x44;

// OHCI register offsets.
const OHCI_HC_REV: u32 = 0x00;
const OHCI_HC_CONTROL: u32 = 0x04;
const OHCI_HC_CMD_STAT: u32 = 0x08;
const _OHCI_HC_INT_STAT: u32 = 0x0C;
const _OHCI_HC_INT_EN: u32 = 0x10;
const _OHCI_HC_RH_STAT: u32 = 0x50;
const OHCI_HC_RH_PORT_STAT: u32 = 0x54;

const _EHCI_CMD_RUN: u32 = 1 << 0;
const EHCI_CMD_RESET: u32 = 1 << 1;
const _EHCI_STS_HCHALTED: u32 = 1 << 12;

#[derive(Clone, Copy, PartialEq)]
enum Kind { Ehci, Ohci }

#[derive(Clone, Copy)]
struct UsbHc {
    base: usize,
    kind: Kind,
    n_ports: u8,
}

static mut G_HC: UsbHc = UsbHc { base: 0, kind: Kind::Ehci, n_ports: 0 };

#[inline]
unsafe fn rd(off: u32) -> u32 {
    Mmio::<u32>::at(G_HC.base + off as usize).read()
}

#[inline]
unsafe fn wr(off: u32, v: u32) {
    Mmio::<u32>::at(G_HC.base + off as usize).write(v);
}

/// Probe for an EHCI controller at the given base.
pub unsafe fn probe_ehci(base: usize) -> bool {
    if base == 0 {
        return false;
    }
    let v = Mmio::<u8>::at(base + EHCI_CAP_VERSION as usize).read();
    // EHCI version high nibble is 1.x.
    (v >> 4) == 1
}

/// Probe for an OHCI controller at the given base.
pub unsafe fn probe_ohci(base: usize) -> bool {
    if base == 0 {
        return false;
    }
    let v = Mmio::<u32>::at(base + OHCI_HC_REV as usize).read();
    // OHCI revision is 0xHHHHHH00 — low byte is 0, top half is 0x10 or 0x11.
    (v & 0xFF) == 0 && ((v >> 16) & 0xFFFF) >= 0x10
}

/// Initialise whichever controller was probed. Programs the operational
/// registers into a known idle state so port enumeration can start.
pub unsafe fn init_ehci(base: usize) -> KResult<()> {
    if !probe_ehci(base) {
        return Err(Errno::NoEnt);
    }
    G_HC = UsbHc { base, kind: Kind::Ehci, n_ports: 0 };
    let cap_len = Mmio::<u8>::at(base + EHCI_CAP_LENGTH as usize).read() as u32;
    let op_base = base + cap_len as usize;
    // Issue reset.
    Mmio::<u32>::at(op_base + EHCI_OP_USBCMD as usize).write(EHCI_CMD_RESET);
    let mut t = 100_000u32;
    while t > 0
        && Mmio::<u32>::at(op_base + EHCI_OP_USBCMD as usize).read() & EHCI_CMD_RESET != 0
    {
        t -= 1;
    }
    // Route all ports to EHCI.
    Mmio::<u32>::at(op_base + EHCI_OP_CONFIGFLAG as usize).write(1);
    // Read port count.
    let hcs = Mmio::<u32>::at(base + EHCI_CAP_HCSPARAMS as usize).read();
    G_HC.n_ports = (hcs & 0xF) as u8;
    Ok(())
}

/// Initialise an OHCI controller.
pub unsafe fn init_ohci(base: usize) -> KResult<()> {
    if !probe_ohci(base) {
        return Err(Errno::NoEnt);
    }
    G_HC = UsbHc { base, kind: Kind::Ohci, n_ports: 0 };
    // Reset: write 0 to HC_CONTROL, then reset.
    wr(OHCI_HC_CONTROL, 0);
    let mut t = 100_000u32;
    while t > 0 && rd(OHCI_HC_CMD_STAT) & 1 != 0 {
        t -= 1;
    }
    Ok(())
}

/// Number of root-hub ports on the active controller.
pub fn n_ports() -> u8 {
    unsafe { G_HC.n_ports }
}

/// Read the status of root-hub port `idx` (0-based).
pub fn port_status(idx: u8) -> KResult<u32> {
    unsafe {
        match G_HC.kind {
            Kind::Ehci => {
                let cap_len = Mmio::<u8>::at(G_HC.base + EHCI_CAP_LENGTH as usize).read() as usize;
                Ok(Mmio::<u32>::at(G_HC.base + cap_len + EHCI_OP_PORTSC as usize + 4 * idx as usize).read())
            }
            Kind::Ohci => {
                if idx >= 15 {
                    return Err(Errno::Range);
                }
                Ok(rd(OHCI_HC_RH_PORT_STAT + 4 * idx as u32))
            }
        }
    }
}

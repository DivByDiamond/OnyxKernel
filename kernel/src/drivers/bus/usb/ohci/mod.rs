pub(super) mod bulk;
pub(super) mod control;
pub(super) mod intr;
pub(super) mod pool;

use crate::arch::mmio::Mmio;

// OHCI register offsets
const OHCI_HC_REV: u32 = 0x00;
const OHCI_HC_CONTROL: u32 = 0x04;
const OHCI_HC_COMMAND_STATUS: u32 = 0x08;
const OHCI_HC_INTERRUPT_STATUS: u32 = 0x0C;
const OHCI_HC_HCCA: u32 = 0x18;
const OHCI_HC_CONTROL_HEAD_ED: u32 = 0x20;
const OHCI_HC_CONTROL_CURRENT_ED: u32 = 0x24;
const OHCI_HC_BULK_HEAD_ED: u32 = 0x28;
const OHCI_HC_BULK_CURRENT_ED: u32 = 0x2C;
const OHCI_HC_FM_INTERVAL: u32 = 0x34;
const OHCI_HC_PERIODIC_START: u32 = 0x40;
const OHCI_HC_LS_THRESHOLD: u32 = 0x44;
const OHCI_HC_RH_DESC_A: u32 = 0x48;
const OHCI_HC_RH_STATUS: u32 = 0x50;
const OHCI_HC_RH_PORT_STATUS: u32 = 0x54;

// OHCI HcControl bits
const OHCI_CTRL_CLE: u32 = 1 << 4;
const OHCI_CTRL_BLE: u32 = 1 << 5;
/// Periodic-list-enable bit, bit 2 (Linux `OHCI_CTRL_PLE`): the HC
/// services the HCCA interrupt lists — harmless over an empty table.
const OHCI_CTRL_PLE: u32 = 1 << 2;
const OHCI_CTRL_HCFS_MASK: u32 = 3 << 6;
const OHCI_CTRL_HCFS_OPER: u32 = 2 << 6;
/// Remote-wakeup-enable bit, bit 10 (Linux `OHCI_CTRL_RWE`); bit 9 is
/// RWC, a read-only status bit.
const OHCI_CTRL_RWE: u32 = 1 << 10;

// OHCI HcCommandStatus bits
const OHCI_CMD_HCR: u32 = 1 << 0;
const OHCI_CMD_CLF: u32 = 1 << 1;

// OHCI Root Hub port status bits
const RH_PS_PES: u32 = 1 << 1;
const RH_PS_PRS: u32 = 1 << 4;
const RH_PS_PPS: u32 = 1 << 8;
const RH_PS_LSDA: u32 = 1 << 9;
const RH_PS_CSC: u32 = 1 << 16;
const RH_PS_PRSC: u32 = 1 << 20;

// OHCI ED word-0 fields (spec 1.0a §4.2.2; cf. Linux ohci.h, QEMU hcd-ohci.c).
const ED_FA_SHIFT: u32 = 0;
/// Endpoint-number field EN, bits 7-10 (QEMU `OHCI_ED_EN_SHIFT`).
const ED_EN_SHIFT: u32 = 7;
/// Speed bit S, bit 13 (Linux `ED_LOWSPEED`); bit 12 is the D field — not speed.
const ED_SPEED_FULL: u32 = 0 << 13;
const ED_SPEED_LOW: u32 = 1 << 13;
/// MPS field, bits 16-26 (QEMU `OHCI_ED_MPS_SHIFT`).
const ED_MPS_SHIFT: u32 = 16;
const ED_TERMINATE: u32 = 1;
/// HeadP data-toggle carry bit (QEMU `OHCI_ED_C`): the poll preserves
/// it across re-arms. (The halt bit ED_H needs no constant — the
/// re-arm's head write clears it by construction.)
const ED_HEAD_CARRY: u32 = 2;

/// TD condition-code field CC, bits 28-31 (Linux `TD_CC = 0xf0000000`).
pub(super) const TD_CC_MASK: u32 = 0xF << 28;
/// HCD marker for a fresh TD (Linux `TD_NOTACCESSED 0x0F`, the
/// HCD-reserved code the HC overwrites on retirement).
pub(super) const TD_CC_NOT_ACCESSED: u32 = 0xF << 28;
/// Condition code of a retired-without-error TD (Linux `TD_CC_NOERROR`).
pub(super) const TD_CC_NOERROR: u32 = 0;
/// Data-toggle field T, bits 24-25 (Linux `TD_T_DATA0 0x02000000`,
/// `TD_T_DATA1 0x03000000`): 10b = DATA0, 11b = DATA1.
pub(super) const TD_T_DATA0: u32 = 2 << 24;
pub(super) const TD_T_DATA1: u32 = 3 << 24;
/// T = 00b: take the data toggle from the ED carry the HC maintains.
pub(super) const TD_T_CARRY: u32 = 0;
/// Delay-interrupt field DI, bits 21-23; 7 = no interrupt for this TD.
pub(super) const TD_DI_NO_INTR: u32 = 7 << 21;
/// Direction/PID field DP, bits 19-20 (Linux `TD_DP_*`).
pub(super) const TD_DP_SETUP: u32 = 0 << 19;
pub(super) const TD_DP_OUT: u32 = 1 << 19;
pub(super) const TD_DP_IN: u32 = 2 << 19;
/// Buffer-rounding bit R, bit 18 (Linux `TD_R`): a short IN packet is
/// not a data-underrun error.
pub(super) const TD_R_ROUND: u32 = 1 << 18;
pub(super) const TD_TERMINATE: u32 = 1;

// OHCI global state
pub(super) static mut G_OHCI_BASE: usize = 0;
pub(super) static mut G_OHCI_N_PORTS: u8 = 0;
/// Physical (identity-mapped) address of the HCCA page `init_ohci`
/// allocated; the periodic interrupt lists live at its start.
static mut G_OHCI_HCCA_PA: u32 = 0;
static mut G_OHCI_HCCA_READY: bool = false;

/// # Safety
///
/// `G_OHCI_BASE` must have been set by `init_ohci` and `reg` must be a
/// register offset from this file's constants.
#[inline]
pub(super) unsafe fn ohci_rd(reg: u32) -> u32 {
    // SAFETY: G_OHCI_BASE was set by init_ohci from the probed controller base; reg is an in-file register offset, so the MMIO read stays in the device window.
    unsafe { Mmio::<u32>::at(G_OHCI_BASE + reg as usize).read() }
}

/// # Safety
///
/// Same contract as `ohci_rd`; the write targets an in-file register offset
/// within the controller's MMIO window.
#[inline]
pub(super) unsafe fn ohci_wr(reg: u32, v: u32) {
    // SAFETY: G_OHCI_BASE was set by init_ohci from the probed controller base; reg is an in-file register offset, so the MMIO write stays in the device window.
    unsafe {
        Mmio::<u32>::at(G_OHCI_BASE + reg as usize).write(v);
    }
}

pub(super) fn ohci_n_ports() -> u8 {
    // SAFETY: plain read of G_OHCI_N_PORTS, written once by init_ohci during single-threaded boot init.
    unsafe { G_OHCI_N_PORTS }
}

pub(super) use bulk::{init_ohci, ohci_bulk_transfer};
pub(super) use control::{
    ohci_control_transfer, ohci_port_enable, ohci_port_reset, ohci_port_speed, ohci_port_status,
    probe_ohci,
};
pub(crate) use intr::{ohci_arm_interrupt, ohci_poll_interrupt};

#[cfg(test)]
mod tests;

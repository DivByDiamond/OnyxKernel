pub(super) mod bulk;
pub(super) mod control;

use crate::arch::mmio::Mmio;
use onyx_core::errno::{Errno, KResult};

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
const OHCI_CTRL_HCFS_MASK: u32 = 3 << 6;
const OHCI_CTRL_HCFS_OPER: u32 = 2 << 6;
const OHCI_CTRL_RWE: u32 = 1 << 9;

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

// OHCI ED/TD bit fields
const ED_FA_SHIFT: u32 = 0;
const ED_SPEED_FULL: u32 = 0 << 12;
const ED_SPEED_LOW: u32 = 1 << 12;
const ED_MPS_SHIFT: u32 = 16;
const ED_TERMINATE: u32 = 1;

pub(super) const TD_CC_MASK: u32 = 0xF;
pub(super) const TD_CC_NOT_ACCESSED: u32 = 0x0F;
pub(super) const TD_T_DATA0: u32 = 0 << 10;
pub(super) const TD_T_DATA1: u32 = 1 << 10;
pub(super) const TD_DI_NO_INTR: u32 = 3 << 11;
pub(super) const TD_DP_SETUP: u32 = 0 << 13;
pub(super) const TD_DP_OUT: u32 = 1 << 13;
pub(super) const TD_DP_IN: u32 = 2 << 13;
pub(super) const TD_R_3: u32 = 1 << 15;
pub(super) const TD_TERMINATE: u32 = 1;

const OHCI_ED_SIZE: usize = 16;
const OHCI_TD_SIZE: usize = 16;

// OHCI data structures
#[repr(C, align(16))]
pub(super) struct OhciED {
    pub(super) control: u32,
    pub(super) tail_td: u32,
    pub(super) head_td: u32,
    pub(super) next_ed: u32,
}

#[repr(C, align(16))]
pub(super) struct OhciTD {
    pub(super) control: u32,
    pub(super) cbp: u32,
    pub(super) next_td: u32,
    pub(super) be: u32,
}

// OHCI global state
pub(super) static mut G_OHCI_BASE: usize = 0;
pub(super) static mut G_OHCI_N_PORTS: u8 = 0;
static mut G_OHCI_HCCA_PA: u32 = 0;
static mut G_OHCI_HCCA_READY: bool = false;

// OHCI DMA pool
const OHCI_DMA_POOL_SIZE: usize = 4096;
const MAX_OHCI_ED: usize = 32;
const MAX_OHCI_TD: usize = 64;

#[repr(C, align(4096))]
struct OhciDmaPool {
    data: [u8; OHCI_DMA_POOL_SIZE],
}
static mut G_OHCI_DMA: OhciDmaPool = OhciDmaPool {
    data: [0; OHCI_DMA_POOL_SIZE],
};
static mut G_OHCI_DMA_USED: usize = 0;
static mut G_OHCI_ED_COUNT: usize = 0;
static mut G_OHCI_TD_COUNT: usize = 0;
static mut G_OHCI_ED_OFFSETS: [usize; MAX_OHCI_ED] = [0; MAX_OHCI_ED];
static mut G_OHCI_TD_OFFSETS: [usize; MAX_OHCI_TD] = [0; MAX_OHCI_TD];

#[inline]
pub(super) unsafe fn ohci_rd(reg: u32) -> u32 {
    Mmio::<u32>::at(G_OHCI_BASE + reg as usize).read()
}

#[inline]
pub(super) unsafe fn ohci_wr(reg: u32, v: u32) {
    Mmio::<u32>::at(G_OHCI_BASE + reg as usize).write(v);
}

unsafe fn ohci_pool_phys(off: usize) -> u32 {
    let pool_va = &raw const G_OHCI_DMA as usize;
    (pool_va + off) as u32
}

unsafe fn ohci_alloc_dma(size: usize) -> KResult<usize> {
    let aligned = (size + 15) & !15;
    let off = G_OHCI_DMA_USED;
    if off + aligned > OHCI_DMA_POOL_SIZE {
        return Err(Errno::NoMem);
    }
    G_OHCI_DMA_USED = off + aligned;
    ::core::ptr::write_bytes(G_OHCI_DMA.data.as_mut_ptr().add(off), 0, aligned);
    Ok(off)
}

pub(super) unsafe fn ohci_alloc_ed() -> KResult<usize> {
    if G_OHCI_ED_COUNT >= MAX_OHCI_ED {
        return Err(Errno::NoMem);
    }
    let off = ohci_alloc_dma(OHCI_ED_SIZE)?;
    let idx = G_OHCI_ED_COUNT;
    G_OHCI_ED_OFFSETS[idx] = off;
    G_OHCI_ED_COUNT += 1;
    Ok(idx)
}

pub(super) unsafe fn ohci_alloc_td() -> KResult<usize> {
    if G_OHCI_TD_COUNT >= MAX_OHCI_TD {
        return Err(Errno::NoMem);
    }
    let off = ohci_alloc_dma(OHCI_TD_SIZE)?;
    let idx = G_OHCI_TD_COUNT;
    G_OHCI_TD_OFFSETS[idx] = off;
    G_OHCI_TD_COUNT += 1;
    Ok(idx)
}

pub(super) unsafe fn ohci_ed_ptr(idx: usize) -> *mut OhciED {
    G_OHCI_DMA.data.as_mut_ptr().add(G_OHCI_ED_OFFSETS[idx]) as *mut OhciED
}

pub(super) unsafe fn ohci_td_ptr(idx: usize) -> *mut OhciTD {
    G_OHCI_DMA.data.as_mut_ptr().add(G_OHCI_TD_OFFSETS[idx]) as *mut OhciTD
}

pub(super) unsafe fn ohci_ed_phys(idx: usize) -> u32 {
    ohci_pool_phys(G_OHCI_ED_OFFSETS[idx])
}

pub(super) unsafe fn ohci_td_phys(idx: usize) -> u32 {
    ohci_pool_phys(G_OHCI_TD_OFFSETS[idx])
}

pub(super) fn ohci_n_ports() -> u8 {
    unsafe { G_OHCI_N_PORTS }
}

pub(super) use bulk::{init_ohci, ohci_bulk_transfer};
pub(super) use control::{
    ohci_control_transfer, ohci_port_enable, ohci_port_reset, ohci_port_speed, ohci_port_status,
    probe_ohci,
};

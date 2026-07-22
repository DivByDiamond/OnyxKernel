pub(super) mod queue;
pub(super) mod xfer;

use crate::arch::mmio::Mmio;
use onyx_core::errno::{Errno, KResult};

pub(super) const EHCI_CAP_LENGTH: u32 = 0x00;
pub(super) const EHCI_CAP_VERSION: u32 = 0x02;
pub(super) const EHCI_CAP_HCSPARAMS: u32 = 0x04;
pub(super) const OP_USBCMD: u32 = 0x00;
pub(super) const OP_USBSTS: u32 = 0x04;
pub(super) const OP_CTRLDSSEGMENT: u32 = 0x10;
pub(super) const OP_ASYNCLISTADDR: u32 = 0x18;
pub(super) const OP_CONFIGFLAG: u32 = 0x40;
pub(super) const OP_PORTSC: u32 = 0x44;
pub(super) const CMD_RUN: u32 = 1 << 0;
pub(super) const CMD_RESET: u32 = 1 << 1;
pub(super) const CMD_ASYNC_ENABLE: u32 = 1 << 5;
pub(super) const STS_HCHALTED: u32 = 1 << 12;
pub(super) const STS_ASYNC_ADVANCE: u32 = 1 << 5;
pub(super) const QTD_ACTIVE: u32 = 1 << 7;
pub(super) const QTD_HALTED: u32 = 1 << 6;
pub(super) const QTD_BUF_ERR: u32 = 1 << 5;
pub(super) const QTD_BABBLE: u32 = 1 << 4;
pub(super) const QTD_XACT_ERR: u32 = 1 << 3;
pub(super) const QTD_ERROR: u32 = QTD_HALTED | QTD_BUF_ERR | QTD_BABBLE | QTD_XACT_ERR;
pub(super) const QTD_PID_OUT: u32 = 0 << 8;
pub(super) const QTD_PID_IN: u32 = 1 << 8;
pub(super) const QTD_PID_SETUP: u32 = 2 << 8;
pub(super) const QTD_CERR_3: u32 = 3 << 10;
pub(super) const QTD_TOGGLE: u32 = 1 << 14;
pub(super) const QTD_TOTAL_LEN_SHIFT: u32 = 16;
pub(super) const QH_DEV_ADDR_SHIFT: u32 = 8;
pub(super) const QH_INACTIVATE: u32 = 1 << 7;
pub(super) const QH_EPS_SHIFT: u32 = 12;
pub(super) const QH_EPS_HIGH: u32 = 2 << QH_EPS_SHIFT;
pub(super) const QH_DTC: u32 = 1 << 14;
pub(super) const QH_HRL: u32 = 1 << 15;
pub(super) const QH_MPL_SHIFT: u32 = 16;
pub(super) const QH_QH: u32 = 0x02;
pub(super) const QH_TERMINATE: u32 = 0x01;
pub(super) const QTD_BUF_SIZE: u32 = 4096;
pub(super) const MAX_QH: usize = 16;
pub(super) const MAX_QTD: usize = 64;
const DMA_POOL_SIZE: usize = 4096;

pub(super) static mut G_OP_BASE: usize = 0;
pub(super) static mut G_N_PORTS: u8 = 0;
pub(super) static mut G_ASYNCLIST_ENABLED: bool = false;

#[repr(C, align(4096))]
struct DmaPool {
    data: [u8; DMA_POOL_SIZE],
}
static mut G_DMA: DmaPool = DmaPool {
    data: [0; DMA_POOL_SIZE],
};
static mut G_DMA_USED: usize = 0;

#[repr(C)]
pub(super) struct QH {
    pub(super) horz_link: u32,
    pub(super) ep_chars: u32,
    pub(super) eps_bits: u32,
    pub(super) current_link: u32,
    pub(super) overlay_next: u32,
    pub(super) overlay_alt_next: u32,
    pub(super) overlay_token: u32,
    pub(super) overlay_buf: [u32; 5],
}
pub(super) const QH_SIZE: usize = ::core::mem::size_of::<QH>();

#[repr(C)]
pub(super) struct QTD {
    pub(super) next: u32,
    pub(super) alt_next: u32,
    pub(super) token: u32,
    pub(super) buf: [u32; 5],
}
pub(super) const QTD_SIZE: usize = ::core::mem::size_of::<QTD>();

pub(super) static mut G_QH_OFFSETS: [usize; MAX_QH] = [0; MAX_QH];
pub(super) static mut G_QH_COUNT: usize = 0;
pub(super) static mut G_QTD_OFFSETS: [usize; MAX_QTD] = [0; MAX_QTD];
pub(super) static mut G_QTD_COUNT: usize = 0;

#[inline]
pub(super) unsafe fn op_rd(reg: u32) -> u32 {
    Mmio::<u32>::at(G_OP_BASE + reg as usize).read()
}
#[inline]
pub(super) unsafe fn op_wr(reg: u32, v: u32) {
    Mmio::<u32>::at(G_OP_BASE + reg as usize).write(v);
}

pub(super) unsafe fn pool_offset_to_phys(off: usize) -> u32 {
    let pool_va = &raw const G_DMA as usize;
    (pool_va + off) as u32
}

pub(super) unsafe fn alloc_dma(size: usize) -> KResult<usize> {
    let aligned = (size + 31) & !31;
    let off = G_DMA_USED;
    if off + aligned > DMA_POOL_SIZE {
        return Err(Errno::NoMem);
    }
    G_DMA_USED = off + aligned;
    ::core::ptr::write_bytes(G_DMA.data.as_mut_ptr().add(off), 0, aligned);
    Ok(off)
}

pub(super) unsafe fn alloc_qh() -> KResult<usize> {
    if G_QH_COUNT >= MAX_QH {
        return Err(Errno::NoMem);
    }
    let off = alloc_dma(QH_SIZE)?;
    let idx = G_QH_COUNT;
    G_QH_OFFSETS[idx] = off;
    G_QH_COUNT += 1;
    Ok(idx)
}

pub(super) unsafe fn alloc_qtd() -> KResult<usize> {
    if G_QTD_COUNT >= MAX_QTD {
        return Err(Errno::NoMem);
    }
    let off = alloc_dma(QTD_SIZE)?;
    let idx = G_QTD_COUNT;
    G_QTD_OFFSETS[idx] = off;
    G_QTD_COUNT += 1;
    Ok(idx)
}

pub(super) unsafe fn qh_ptr(idx: usize) -> *mut QH {
    G_DMA.data.as_mut_ptr().add(G_QH_OFFSETS[idx]) as *mut QH
}

pub(super) unsafe fn qtd_ptr(idx: usize) -> *mut QTD {
    G_DMA.data.as_mut_ptr().add(G_QTD_OFFSETS[idx]) as *mut QTD
}

pub(super) unsafe fn qh_phys(idx: usize) -> u32 {
    pool_offset_to_phys(G_QH_OFFSETS[idx])
}

pub(super) unsafe fn qtd_phys(idx: usize) -> u32 {
    pool_offset_to_phys(G_QTD_OFFSETS[idx])
}

pub(super) unsafe fn probe_ehci(base: usize) -> bool {
    if base == 0 {
        return false;
    }
    let cl = Mmio::<u32>::at(base).read();
    let v = Mmio::<u32>::at(base + EHCI_CAP_VERSION as usize).read();
    (v & 0xFF) >= 0x10 && (cl & 0xFF) > 0
}

pub(super) fn ehci_n_ports() -> u8 {
    unsafe { G_N_PORTS }
}

pub(super) unsafe fn ehci_port_status(idx: u8) -> KResult<u32> {
    if idx >= G_N_PORTS {
        return Err(Errno::Range);
    }
    let reg = OP_PORTSC + 4 * idx as u32;
    Ok(op_rd(reg))
}

pub(super) unsafe fn ehci_port_reset(idx: u8) -> KResult<()> {
    if idx >= G_N_PORTS {
        return Err(Errno::Range);
    }
    let reg = OP_PORTSC + 4 * idx as u32;
    op_wr(reg, op_rd(reg) | (1 << 8));
    let mut timeout = 100_000u32;
    while timeout > 0 && (op_rd(reg) & (1 << 8)) != 0 {
        timeout -= 1;
    }
    if timeout == 0 {
        return Err(Errno::Io);
    }
    Ok(())
}

pub(super) unsafe fn ehci_port_enable(idx: u8) -> KResult<()> {
    if idx >= G_N_PORTS {
        return Err(Errno::Range);
    }
    let reg = OP_PORTSC + 4 * idx as u32;
    op_wr(reg, op_rd(reg) | (1 << 4));
    Ok(())
}

pub(super) use xfer::bulk::ehci_bulk_transfer;
pub(super) use xfer::ehci_control_transfer;

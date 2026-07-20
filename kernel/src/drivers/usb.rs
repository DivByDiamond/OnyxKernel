//! USB host controller — EHCI async schedule + xHCI driver.
//!
//! Provides EHCI async list scheduler and xHCI controller driver
//! with mass storage class support.
pub mod core;
pub mod hcd_ehci;
pub mod hcd_ohci;
pub mod xhci;

use crate::arch::mmio::Mmio;
use crate::mm::pmm;
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

// OHCI register offsets.
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

// OHCI HcControl bits.
const OHCI_CTRL_CLE: u32 = 1 << 4;
const OHCI_CTRL_BLE: u32 = 1 << 5;
const OHCI_CTRL_HCFS_MASK: u32 = 3 << 6;
const OHCI_CTRL_HCFS_OPER: u32 = 2 << 6;
const OHCI_CTRL_RWE: u32 = 1 << 9;

// OHCI HcCommandStatus bits.
const OHCI_CMD_HCR: u32 = 1 << 0;
const OHCI_CMD_CLF: u32 = 1 << 1;

// OHCI interrupt bits.

// OHCI Root Hub port status bits.
const RH_PS_PES: u32 = 1 << 1;
const RH_PS_PRS: u32 = 1 << 4;
const RH_PS_PPS: u32 = 1 << 8;
const RH_PS_LSDA: u32 = 1 << 9;
const RH_PS_CSC: u32 = 1 << 16;
const RH_PS_PRSC: u32 = 1 << 20;

// OHCI ED/TD bit fields.
const ED_FA_SHIFT: u32 = 0;
const ED_SPEED_FULL: u32 = 0 << 12;
const ED_SPEED_LOW: u32 = 1 << 12;
const ED_MPS_SHIFT: u32 = 16;
const ED_TERMINATE: u32 = 1;

const TD_CC_MASK: u32 = 0xF;
const TD_CC_NOT_ACCESSED: u32 = 0x0F;
const TD_T_DATA0: u32 = 0 << 10;
const TD_T_DATA1: u32 = 1 << 10;
const TD_DI_NO_INTR: u32 = 3 << 11;
const TD_DP_SETUP: u32 = 0 << 13;
const TD_DP_OUT: u32 = 1 << 13;
const TD_DP_IN: u32 = 2 << 13;
const TD_R_3: u32 = 1 << 15;
const TD_TERMINATE: u32 = 1;

const OHCI_ED_SIZE: usize = 16;
const OHCI_TD_SIZE: usize = 16;

// EHCI capability register offsets.
const EHCI_CAP_LENGTH: u32 = 0x00;
const EHCI_CAP_VERSION: u32 = 0x02;
const EHCI_CAP_HCSPARAMS: u32 = 0x04;

// EHCI operational register offsets (relative to op_base).
const OP_USBCMD: u32 = 0x00;
const OP_USBSTS: u32 = 0x04;
const OP_CTRLDSSEGMENT: u32 = 0x10;
const OP_ASYNCLISTADDR: u32 = 0x18;
const OP_CONFIGFLAG: u32 = 0x40;
const OP_PORTSC: u32 = 0x44;

// USBCMD bits.
const CMD_RUN: u32 = 1 << 0;
const CMD_RESET: u32 = 1 << 1;
const CMD_ASYNC_ENABLE: u32 = 1 << 5;

// USBSTS bits.
const STS_HCHALTED: u32 = 1 << 12;
const STS_ASYNC_ADVANCE: u32 = 1 << 5;

// qTD token bits.
const QTD_ACTIVE: u32 = 1 << 7;
const QTD_HALTED: u32 = 1 << 6;
const QTD_BUF_ERR: u32 = 1 << 5;
const QTD_BABBLE: u32 = 1 << 4;
const QTD_XACT_ERR: u32 = 1 << 3;
const QTD_ERROR: u32 = QTD_HALTED | QTD_BUF_ERR | QTD_BABBLE | QTD_XACT_ERR;
const QTD_PID_OUT: u32 = 0 << 8;
const QTD_PID_IN: u32 = 1 << 8;
const QTD_PID_SETUP: u32 = 2 << 8;
const QTD_CERR_3: u32 = 3 << 10;
const QTD_TOGGLE: u32 = 1 << 14;
const QTD_TOTAL_LEN_SHIFT: u32 = 16;

// QH endpoint chars bits.
const QH_DEV_ADDR_SHIFT: u32 = 8;
const QH_INACTIVATE: u32 = 1 << 7;
const QH_EPS_SHIFT: u32 = 12;
const QH_EPS_HIGH: u32 = 2 << QH_EPS_SHIFT;
const QH_DTC: u32 = 1 << 14;
const QH_HRL: u32 = 1 << 15;
const QH_MPL_SHIFT: u32 = 16;
const QH_QH: u32 = 0x02;
const QH_TERMINATE: u32 = 0x01;

// qTD buffer size per page.
const QTD_BUF_SIZE: u32 = 4096;

// Max endpoints we can track.
const MAX_QH: usize = 16;
const MAX_QTD: usize = 64;
const DMA_POOL_SIZE: usize = 4096;

// Global state.
static mut G_OP_BASE: usize = 0;
static mut G_N_PORTS: u8 = 0;
static mut G_ASYNCLIST_ENABLED: bool = false;

// DMA pool for QH/qTD — must be page-aligned so physical = virtual.
#[repr(C, align(4096))]
struct DmaPool {
    data: [u8; DMA_POOL_SIZE],
}
static mut G_DMA: DmaPool = DmaPool {
    data: [0; DMA_POOL_SIZE],
};
static mut G_DMA_USED: usize = 0;

// OHCI data structures.
#[repr(C, align(16))]
struct OhciED {
    control: u32,
    tail_td: u32,
    head_td: u32,
    next_ed: u32,
}

#[repr(C, align(16))]
struct OhciTD {
    control: u32,
    cbp: u32,
    next_td: u32,
    be: u32,
}

// OHCI global state.
static mut G_OHCI_BASE: usize = 0;
static mut G_OHCI_N_PORTS: u8 = 0;
static mut G_OHCI_HCCA_PA: u32 = 0;
static mut G_OHCI_HCCA_READY: bool = false;

// OHCI DMA pool — separate static pool matching the EHCI pattern.
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

// QH and qTD structures at known offsets in the DMA pool.
// We embed a fixed number statically so the physical offset is known.
#[repr(C)]
struct QH {
    horz_link: u32,
    ep_chars: u32,
    eps_bits: u32,
    current_link: u32,
    overlay_next: u32,
    overlay_alt_next: u32,
    overlay_token: u32,
    overlay_buf: [u32; 5],
}
const QH_SIZE: usize = ::core::mem::size_of::<QH>();

#[repr(C)]
struct QTD {
    next: u32,
    alt_next: u32,
    token: u32,
    buf: [u32; 5],
}
const QTD_SIZE: usize = ::core::mem::size_of::<QTD>();

static mut G_QH_OFFSETS: [usize; MAX_QH] = [0; MAX_QH];
static mut G_QH_COUNT: usize = 0;
static mut G_QTD_OFFSETS: [usize; MAX_QTD] = [0; MAX_QTD];
static mut G_QTD_COUNT: usize = 0;

#[inline]
unsafe fn op_rd(reg: u32) -> u32 {
    Mmio::<u32>::at(G_OP_BASE + reg as usize).read()
}
#[inline]
unsafe fn op_wr(reg: u32, v: u32) {
    Mmio::<u32>::at(G_OP_BASE + reg as usize).write(v);
}

/// Convert a DMA pool offset to a physical bus address.
/// The pool is identity-mapped at its virtual address.
unsafe fn pool_offset_to_phys(off: usize) -> u32 {
    let pool_va = &raw const G_DMA as usize;
    (pool_va + off) as u32
}

unsafe fn alloc_dma(size: usize) -> KResult<usize> {
    let aligned = (size + 31) & !31;
    let off = G_DMA_USED;
    if off + aligned > DMA_POOL_SIZE {
        return Err(Errno::NoMem);
    }
    G_DMA_USED = off + aligned;
    ::core::ptr::write_bytes(G_DMA.data.as_mut_ptr().add(off), 0, aligned);
    Ok(off)
}

unsafe fn alloc_qh() -> KResult<usize> {
    if G_QH_COUNT >= MAX_QH {
        return Err(Errno::NoMem);
    }
    let off = alloc_dma(QH_SIZE)?;
    let idx = G_QH_COUNT;
    G_QH_OFFSETS[idx] = off;
    G_QH_COUNT += 1;
    Ok(idx)
}

unsafe fn alloc_qtd() -> KResult<usize> {
    if G_QTD_COUNT >= MAX_QTD {
        return Err(Errno::NoMem);
    }
    let off = alloc_dma(QTD_SIZE)?;
    let idx = G_QTD_COUNT;
    G_QTD_OFFSETS[idx] = off;
    G_QTD_COUNT += 1;
    Ok(idx)
}

unsafe fn qh_ptr(idx: usize) -> *mut QH {
    let off = G_QH_OFFSETS[idx];
    G_DMA.data.as_mut_ptr().add(off) as *mut QH
}

unsafe fn qtd_ptr(idx: usize) -> *mut QTD {
    let off = G_QTD_OFFSETS[idx];
    G_DMA.data.as_mut_ptr().add(off) as *mut QTD
}

unsafe fn qh_phys(idx: usize) -> u32 {
    pool_offset_to_phys(G_QH_OFFSETS[idx])
}

unsafe fn qtd_phys(idx: usize) -> u32 {
    pool_offset_to_phys(G_QTD_OFFSETS[idx])
}

// ─── OHCI MMIO accessors ───────────────────────────────────────────────────

#[inline]
unsafe fn ohci_rd(reg: u32) -> u32 {
    Mmio::<u32>::at(G_OHCI_BASE + reg as usize).read()
}

#[inline]
unsafe fn ohci_wr(reg: u32, v: u32) {
    Mmio::<u32>::at(G_OHCI_BASE + reg as usize).write(v);
}

// ─── OHCI DMA pool helpers ─────────────────────────────────────────────────

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

unsafe fn ohci_alloc_ed() -> KResult<usize> {
    if G_OHCI_ED_COUNT >= MAX_OHCI_ED {
        return Err(Errno::NoMem);
    }
    let off = ohci_alloc_dma(OHCI_ED_SIZE)?;
    let idx = G_OHCI_ED_COUNT;
    G_OHCI_ED_OFFSETS[idx] = off;
    G_OHCI_ED_COUNT += 1;
    Ok(idx)
}

unsafe fn ohci_alloc_td() -> KResult<usize> {
    if G_OHCI_TD_COUNT >= MAX_OHCI_TD {
        return Err(Errno::NoMem);
    }
    let off = ohci_alloc_dma(OHCI_TD_SIZE)?;
    let idx = G_OHCI_TD_COUNT;
    G_OHCI_TD_OFFSETS[idx] = off;
    G_OHCI_TD_COUNT += 1;
    Ok(idx)
}

unsafe fn ohci_ed_ptr(idx: usize) -> *mut OhciED {
    G_OHCI_DMA.data.as_mut_ptr().add(G_OHCI_ED_OFFSETS[idx]) as *mut OhciED
}

unsafe fn ohci_td_ptr(idx: usize) -> *mut OhciTD {
    G_OHCI_DMA.data.as_mut_ptr().add(G_OHCI_TD_OFFSETS[idx]) as *mut OhciTD
}

unsafe fn ohci_ed_phys(idx: usize) -> u32 {
    ohci_pool_phys(G_OHCI_ED_OFFSETS[idx])
}

unsafe fn ohci_td_phys(idx: usize) -> u32 {
    ohci_pool_phys(G_OHCI_TD_OFFSETS[idx])
}

/// Initialise the async list with a dummy QH (reclamation head).
/// The async list is a circular linked list: the last QH points back to
/// the head, and the head has bit 0 set (type=QH) and bit 1 clear.
unsafe fn init_async_list() -> KResult<()> {
    if G_ASYNCLIST_ENABLED {
        return Ok(());
    }
    let head_idx = alloc_qh()?;
    let head = qh_ptr(head_idx);
    let head_phys = qh_phys(head_idx);
    // The reclamation head: horz_link is terminate (pointing to itself
    // in a circular list). We set bit 0 (QH type) but NOT bit 1 (not head).
    // Actually per EHCI spec: the async list head QH should have H bit (bit 1)
    // set in its ep_chars to indicate it's the reclamation head.
    (*head).horz_link = head_phys | QH_QH;
    (*head).ep_chars = QH_HRL | QH_INACTIVATE;
    (*head).eps_bits = 0;
    (*head).current_link = 0;
    // Init the overlay to inactive.
    (*head).overlay_next = QH_TERMINATE;
    (*head).overlay_alt_next = QH_TERMINATE;
    (*head).overlay_token = 0;
    // Point ASYNCLISTADDR to the head QH.
    op_wr(OP_ASYNCLISTADDR, head_phys);
    // Enable async schedule.
    op_wr(OP_USBCMD, op_rd(OP_USBCMD) | CMD_ASYNC_ENABLE);
    // Wait for async schedule to start.
    let mut timeout = 1000u32;
    while timeout > 0 && (op_rd(OP_USBSTS) & STS_HCHALTED) != 0 {
        timeout -= 1;
    }
    G_ASYNCLIST_ENABLED = true;
    Ok(())
}

/// Insert a QH after the async list head.
/// The new QH's horz_link points to the head, and the head's horz_link
/// points to the new QH, maintaining the circular list.
unsafe fn qh_insert(idx: usize) {
    if !G_ASYNCLIST_ENABLED {
        return;
    }
    let head_phys = op_rd(OP_ASYNCLISTADDR) & !0x1F;
    let qh = qh_ptr(idx);
    let qh_phys_addr = qh_phys(idx);
    // Link the new QH to point to the head (circular).
    (*qh).horz_link = head_phys | QH_QH;
    // Ensure the QH is not static (i.e., do NOT set H bit).
    // Set the head of the async list to point to the new QH.
    let head = (head_phys as usize) as *mut QH;
    let _old_next = (*head).horz_link;
    (*head).horz_link = qh_phys_addr | QH_QH;
    // Check for async advance (optional — for active schedules).
    if (op_rd(OP_USBSTS) & STS_ASYNC_ADVANCE) != 0 {
        op_wr(OP_USBSTS, STS_ASYNC_ADVANCE);
    }
}

unsafe fn qh_remove(idx: usize) {
    if !G_ASYNCLIST_ENABLED {
        return;
    }
    let head_phys = op_rd(OP_ASYNCLISTADDR) & !0x1F;
    let qh_phys_addr = qh_phys(idx);
    // Walk the async list from the head to find the QH before ours.
    let mut prev_phys = head_phys;
    loop {
        let prev = (prev_phys as usize) as *const QH;
        let next = (*prev).horz_link & !0x1F;
        if next == qh_phys_addr {
            // Unlink: prev->horz_link = qh->horz_link
            let qh = qh_ptr(idx);
            let qh_next = (*qh).horz_link;
            let prev_mut = prev as *mut QH;
            (*prev_mut).horz_link = qh_next;
            break;
        }
        if next == 0 || next == head_phys {
            break;
        }
        prev_phys = next;
    }
}

/// Submit a control transfer (setup + optional data + status) and wait.
/// `dev_addr`: USB device address (0 for initial enumeration).
/// `setup_pkt`: 8-byte setup packet.
/// `data`: optional data buffer (IN or OUT).
/// `data_dir`: true = IN (device to host), false = OUT (host to device).
/// `max_pkt`: max packet size for endpoint 0.
/// Returns the number of bytes transferred.
/// Submit a control transfer (setup + optional data + status) and wait.
/// `dev_addr`: USB device address (0 for initial enumeration).
/// `setup_pkt`: 8-byte setup packet (must be in identity-mapped memory).
/// `data`: optional data buffer (IN or OUT, must be identity-mapped).
/// `data_in`: true = IN (device to host), false = OUT (host to device).
/// `max_pkt`: max packet size for endpoint 0.
/// Returns the number of bytes transferred.
unsafe fn ehci_control_transfer(
    dev_addr: u8,
    setup_pkt: &[u8; 8],
    mut data: Option<&mut [u8]>,
    data_in: bool,
    max_pkt: u32,
) -> KResult<u32> {
    if !G_ASYNCLIST_ENABLED {
        init_async_list()?;
    }

    let data_len = data.as_ref().map(|d| d.len() as u32).unwrap_or(0);
    let data_qtds = if data_len > 0 {
        (data_len + QTD_BUF_SIZE - 1) / QTD_BUF_SIZE
    } else {
        0
    };
    let qtd_count = 1 + data_qtds + 1; // setup + data + status

    if qtd_count > MAX_QTD as u32 {
        return Err(Errno::NoMem);
    }

    // Allocate a QH for this transfer.
    let qh_idx = alloc_qh()?;
    let qh = qh_ptr(qh_idx);
    let setup_pid = QTD_PID_SETUP;
    let data_pid = if data_in { QTD_PID_IN } else { QTD_PID_OUT };

    // Allocate qTDs.
    let mut qtd_indices = [0usize; 64];
    for i in 0..qtd_count as usize {
        qtd_indices[i] = alloc_qtd()?;
    }

    // Setup qTD (first).
    let sqtd = qtd_ptr(qtd_indices[0]);
    let setup_phys = setup_pkt.as_ptr() as u32;
    let next_phys = if qtd_count > 1 {
        qtd_phys(qtd_indices[1])
    } else {
        QH_TERMINATE
    };
    (*sqtd).next = next_phys;
    (*sqtd).alt_next = QH_TERMINATE;
    (*sqtd).token = setup_pid | QTD_CERR_3 | (8 << QTD_TOTAL_LEN_SHIFT) | QTD_ACTIVE;
    (*sqtd).buf = [setup_phys, 0, 0, 0, 0];

    // Data qTDs.
    let mut buf_pos = 0u32;
    for i in 0..data_qtds as usize {
        let dqtd = qtd_ptr(qtd_indices[1 + i]);
        let remaining = data_len - buf_pos;
        let chunk = remaining.min(QTD_BUF_SIZE);
        let buf_phys = if let Some(ref mut db) = data {
            db.as_mut_ptr().add(buf_pos as usize) as u32
        } else {
            0
        };
        let next_d = if i + 1 < data_qtds as usize {
            qtd_phys(qtd_indices[1 + i + 1])
        } else {
            qtd_phys(qtd_indices[qtd_count as usize - 1])
        };
        (*dqtd).next = if data_len > 0 { next_d } else { QH_TERMINATE };
        (*dqtd).alt_next = QH_TERMINATE;
        (*dqtd).token = data_pid | QTD_CERR_3 | (chunk << QTD_TOTAL_LEN_SHIFT) | QTD_ACTIVE;
        (*dqtd).buf = [buf_phys, 0, 0, 0, 0];
        buf_pos += chunk;
    }

    // Status qTD (last): opposite direction, 0 bytes.
    let status_pid = if data_in || data_len == 0 {
        QTD_PID_OUT
    } else {
        QTD_PID_IN
    };
    let status_idx = qtd_count as usize - 1;
    let stqtd = qtd_ptr(qtd_indices[status_idx]);
    (*stqtd).next = QH_TERMINATE;
    (*stqtd).alt_next = QH_TERMINATE;
    (*stqtd).token = status_pid | QTD_CERR_3 | QTD_TOGGLE | QTD_ACTIVE;
    (*stqtd).buf = [0, 0, 0, 0, 0];

    // Configure the QH for this transfer.
    let mpl_val = max_pkt.min(8) << QH_MPL_SHIFT;
    let eps = QH_EPS_HIGH;
    let dev_addr_bits = (dev_addr as u32) << QH_DEV_ADDR_SHIFT;
    (*qh).horz_link = 0;
    (*qh).ep_chars = dev_addr_bits | eps | QH_DTC | mpl_val;
    (*qh).eps_bits = 0;
    (*qh).current_link = qtd_phys(qtd_indices[0]);
    (*qh).overlay_next = qtd_phys(qtd_indices[0]);
    (*qh).overlay_alt_next = QH_TERMINATE;
    (*qh).overlay_token = setup_pid | QTD_CERR_3 | (8 << QTD_TOTAL_LEN_SHIFT) | QTD_ACTIVE;
    (*qh).overlay_buf = [setup_phys, 0, 0, 0, 0];

    // Insert QH into async list and doorbell.
    qh_insert(qh_idx);

    // Poll for completion.
    let mut timeout = 500_000u32;
    let mut bytes_xfered = 0u32;
    loop {
        let status = op_rd(OP_USBSTS);
        if (status & STS_HCHALTED) != 0 {
            qh_remove(qh_idx);
            return Err(Errno::Io);
        }
        let token = (*sqtd).token;
        if (token & QTD_ACTIVE) == 0 {
            if (token & QTD_ERROR) != 0 {
                qh_remove(qh_idx);
                return Err(Errno::Io);
            }
            let mut all_done = true;
            for i in 0..data_qtds as usize {
                let dqtd = qtd_ptr(qtd_indices[1 + i]);
                let dt = (*dqtd).token;
                if (dt & QTD_ACTIVE) != 0 {
                    all_done = false;
                    break;
                }
                if (dt & QTD_ERROR) != 0 {
                    qh_remove(qh_idx);
                    return Err(Errno::Io);
                }
                let _dlen = (*dqtd).token >> QTD_TOTAL_LEN_SHIFT;
                bytes_xfered = bytes_xfered.max(buf_pos.min(data_len));
            }
            if all_done {
                let stoken = (*stqtd).token;
                if (stoken & QTD_ACTIVE) == 0 && (stoken & QTD_ERROR) == 0 {
                    qh_remove(qh_idx);
                    return Ok(bytes_xfered);
                }
            }
        }
        if timeout == 0 {
            qh_remove(qh_idx);
            return Err(Errno::Io);
        }
        timeout -= 1;
    }
}

/// Submit a bulk transfer (no setup/status stages, just data).
pub unsafe fn ehci_bulk_transfer(
    dev_addr: u8,
    mut data: Option<&mut [u8]>,
    data_in: bool,
    max_pkt: u32,
) -> KResult<u32> {
    if !G_ASYNCLIST_ENABLED {
        init_async_list()?;
    }

    let data_len = data.as_ref().map(|d| d.len() as u32).unwrap_or(0);
    if data_len == 0 {
        return Ok(0);
    }
    let data_qtds = (data_len + QTD_BUF_SIZE - 1) / QTD_BUF_SIZE;
    if data_qtds > MAX_QTD as u32 {
        return Err(Errno::NoMem);
    }

    let qh_idx = alloc_qh()?;
    let qh = qh_ptr(qh_idx);
    let data_pid = if data_in { QTD_PID_IN } else { QTD_PID_OUT };

    let mut qtd_indices = [0usize; 64];
    for i in 0..data_qtds as usize {
        qtd_indices[i] = alloc_qtd()?;
    }

    let mut buf_pos = 0u32;
    for i in 0..data_qtds as usize {
        let dqtd = qtd_ptr(qtd_indices[i]);
        let remaining = data_len - buf_pos;
        let chunk = remaining.min(QTD_BUF_SIZE);
        let buf_phys = if let Some(ref mut db) = data {
            db.as_mut_ptr().add(buf_pos as usize) as u32
        } else {
            0
        };
        let next_d = if i + 1 < data_qtds as usize {
            qtd_phys(qtd_indices[i + 1])
        } else {
            QH_TERMINATE
        };
        (*dqtd).next = next_d;
        (*dqtd).alt_next = QH_TERMINATE;
        (*dqtd).token = data_pid | QTD_CERR_3 | (chunk << QTD_TOTAL_LEN_SHIFT) | QTD_ACTIVE;
        (*dqtd).buf = [buf_phys, 0, 0, 0, 0];
        buf_pos += chunk;
    }
    let mpl_val = max_pkt.min(512) << QH_MPL_SHIFT;
    let eps = QH_EPS_HIGH;
    let dev_addr_bits = (dev_addr as u32) << QH_DEV_ADDR_SHIFT;
    (*qh).horz_link = 0;
    (*qh).ep_chars = dev_addr_bits | eps | QH_DTC | mpl_val;
    (*qh).eps_bits = 0;
    (*qh).current_link = qtd_phys(qtd_indices[0]);
    (*qh).overlay_next = qtd_phys(qtd_indices[0]);
    (*qh).overlay_alt_next = QH_TERMINATE;
    (*qh).overlay_token =
        data_pid | QTD_CERR_3 | (data_len.min(QTD_BUF_SIZE) << QTD_TOTAL_LEN_SHIFT) | QTD_ACTIVE;
    (*qh).overlay_buf = [0, 0, 0, 0, 0];

    qh_insert(qh_idx);

    let mut timeout = 500_000u32;
    let mut bytes_xfered = 0u32;
    loop {
        let status = op_rd(OP_USBSTS);
        if (status & STS_HCHALTED) != 0 {
            qh_remove(qh_idx);
            return Err(Errno::Io);
        }
        let mut all_done = true;
        for i in 0..data_qtds as usize {
            let dqtd = qtd_ptr(qtd_indices[i]);
            let dt = (*dqtd).token;
            if (dt & QTD_ACTIVE) != 0 {
                all_done = false;
                break;
            }
            if (dt & QTD_ERROR) != 0 {
                qh_remove(qh_idx);
                return Err(Errno::Io);
            }
            let remain = dt >> QTD_TOTAL_LEN_SHIFT;
            let this_chunk = if i + 1 < data_qtds as usize {
                QTD_BUF_SIZE
            } else {
                data_len - (data_qtds - 1) * QTD_BUF_SIZE
            };
            bytes_xfered += this_chunk - remain;
        }
        if all_done {
            qh_remove(qh_idx);
            return Ok(bytes_xfered);
        }
        if timeout == 0 {
            qh_remove(qh_idx);
            return Err(Errno::Io);
        }
        timeout -= 1;
    }
}

/// Probe for an EHCI controller at the given base.
pub unsafe fn probe_ehci(base: usize) -> bool {
    if base == 0 {
        return false;
    }
    let v = Mmio::<u8>::at(base + EHCI_CAP_VERSION as usize).read();
    (v >> 4) == 1
}

/// Probe for an OHCI controller at the given base.
pub unsafe fn probe_ohci(base: usize) -> bool {
    if base == 0 {
        return false;
    }
    let v = Mmio::<u32>::at(base + OHCI_HC_REV as usize).read();
    (v & 0xFF) == 0 && ((v >> 16) & 0xFFFF) >= 0x10
}

/// Initialise EHCI: reset, set base, enable ports, init async list.
pub unsafe fn init_ehci(base: usize) -> KResult<()> {
    if !probe_ehci(base) {
        return Err(Errno::NoEnt);
    }
    let cap_len = Mmio::<u8>::at(base + EHCI_CAP_LENGTH as usize).read() as usize;
    G_OP_BASE = base + cap_len;
    let hcs = Mmio::<u32>::at(base + EHCI_CAP_HCSPARAMS as usize).read();
    G_N_PORTS = (hcs & 0xF) as u8;
    G_ASYNCLIST_ENABLED = false;
    G_DMA_USED = 0;
    G_QH_COUNT = 0;
    G_QTD_COUNT = 0;

    // Reset controller.
    op_wr(OP_USBCMD, CMD_RESET);
    let mut timeout = 100_000u32;
    while timeout > 0 && (op_rd(OP_USBCMD) & CMD_RESET) != 0 {
        timeout -= 1;
    }
    if timeout == 0 {
        return Err(Errno::Io);
    }

    // Set 64-bit segment (0 for 32-bit).
    op_wr(OP_CTRLDSSEGMENT, 0);

    // Route all ports to EHCI.
    op_wr(OP_CONFIGFLAG, 1);

    // Start the controller.
    op_wr(OP_USBCMD, CMD_RUN);

    // Wait for halt to clear.
    timeout = 100_000u32;
    while timeout > 0 && (op_rd(OP_USBSTS) & STS_HCHALTED) != 0 {
        timeout -= 1;
    }
    if timeout == 0 {
        return Err(Errno::Io);
    }

    // Init async list (allocates QH head + enables async schedule).
    init_async_list()?;

    G_ACTIVE = ControllerType::Ehci;
    Ok(())
}

/// ─── OHCI driver ──────────────────────────────────────────────────────────

/// Initialise OHCI: reset, set operational, configure root hub ports.
pub unsafe fn init_ohci(base: usize) -> KResult<()> {
    if !probe_ohci(base) {
        return Err(Errno::NoEnt);
    }

    G_OHCI_BASE = base;

    // Read port count from HcRhDescriptorA (bits 7:0 = NDP).
    let desc_a = ohci_rd(OHCI_HC_RH_DESC_A);
    G_OHCI_N_PORTS = (desc_a & 0xFF) as u8;

    // Allocate HCCA via PMM (4KB page, identity-mapped, 256-byte aligned).
    let hcca_pa = pmm::alloc_zero()? as u32;
    G_OHCI_HCCA_PA = hcca_pa;
    G_OHCI_HCCA_READY = true;

    // Write HCCA physical address.
    ohci_wr(OHCI_HC_HCCA, hcca_pa);

    // Reset the controller: set HCR in HcCommandStatus, wait for self-clear.
    ohci_wr(OHCI_HC_COMMAND_STATUS, OHCI_CMD_HCR);
    let mut timeout = 100_000u32;
    while timeout > 0 && (ohci_rd(OHCI_HC_COMMAND_STATUS) & OHCI_CMD_HCR) != 0 {
        timeout -= 1;
    }
    if timeout == 0 {
        G_OHCI_HCCA_READY = false;
        return Err(Errno::Io);
    }

    // Read default FmInterval from hardware and write it back.
    let fi = ohci_rd(OHCI_HC_FM_INTERVAL);
    // Ensure FSMPS (bits 31:16) is set correctly: FSMPS = FI * 90%.
    let fi_fit = fi & 0x3FFF; // bits 13:0 = FrameInterval
    let fsmps = ((fi_fit * 9) / 10) << 16;
    ohci_wr(OHCI_HC_FM_INTERVAL, (fi & 0xFFFF) | fsmps);

    // Write PeriodicStart = FmInterval * 90%.
    let periodic_start = (fi_fit * 9) / 10;
    ohci_wr(OHCI_HC_PERIODIC_START, periodic_start);

    // Write LSThreshold = 0x628.
    ohci_wr(OHCI_HC_LS_THRESHOLD, 0x628);

    // Clear control/bulk head ED and current ED pointers.
    ohci_wr(OHCI_HC_CONTROL_HEAD_ED, 0);
    ohci_wr(OHCI_HC_CONTROL_CURRENT_ED, 0);
    ohci_wr(OHCI_HC_BULK_HEAD_ED, 0);
    ohci_wr(OHCI_HC_BULK_CURRENT_ED, 0);

    // Clear all interrupt status by writing all 1s.
    ohci_wr(OHCI_HC_INTERRUPT_STATUS, 0xFFFF_FFFF);

    // Set HCFS to Operational with CLE|BLE and RWE.
    ohci_wr(
        OHCI_HC_CONTROL,
        OHCI_CTRL_HCFS_OPER | OHCI_CTRL_CLE | OHCI_CTRL_BLE | OHCI_CTRL_RWE,
    );

    // Wait for controller to become operational.
    timeout = 100_000u32;
    while timeout > 0 && (ohci_rd(OHCI_HC_CONTROL) & OHCI_CTRL_HCFS_MASK) != OHCI_CTRL_HCFS_OPER {
        timeout -= 1;
    }
    if timeout == 0 {
        G_OHCI_HCCA_READY = false;
        return Err(Errno::Io);
    }

    // Set global power on root hub ports.
    ohci_wr(OHCI_HC_RH_STATUS, 1 << 16); // LPSC (Set Global Power)

    // Enable power on each port individually.
    for i in 0..G_OHCI_N_PORTS {
        let port_reg = OHCI_HC_RH_PORT_STATUS + 4 * i as u32;
        let ps = ohci_rd(port_reg);
        ohci_wr(port_reg, ps | RH_PS_PPS);
    }

    // Clear port status changes.
    for i in 0..G_OHCI_N_PORTS {
        let port_reg = OHCI_HC_RH_PORT_STATUS + 4 * i as u32;
        ohci_wr(port_reg, RH_PS_CSC | RH_PS_PRSC);
    }

    G_ACTIVE = ControllerType::Ohci;
    Ok(())
}

/// Perform an OHCI control transfer (setup + optional data + status phase).
/// `speed`: 0 = Full Speed, 1 = Low Speed.
pub unsafe fn ohci_control_transfer(
    dev_addr: u8,
    setup_pkt: &[u8; 8],
    mut data: Option<&mut [u8]>,
    data_in: bool,
    max_pkt: u32,
    speed: u8,
) -> KResult<u32> {
    let data_len = data.as_ref().map(|d| d.len() as u32).unwrap_or(0);
    let total_tds = 1 + 1; // setup + status (+ merged data TD)
    // Use a single data TD that can span the full buffer (OHCI has no 4KB limit per TD).

    if total_tds + if data_len > 0 { 1 } else { 0 } > MAX_OHCI_TD as u32 {
        return Err(Errno::NoMem);
    }

    // Allocate ED.
    let ed_idx = ohci_alloc_ed()?;
    let setup_td_idx = ohci_alloc_td()?;
    let data_td_idx = if data_len > 0 {
        Some(ohci_alloc_td()?)
    } else {
        None
    };
    let status_td_idx = ohci_alloc_td()?;

    let ed = ohci_ed_ptr(ed_idx);
    let setup_td = ohci_td_ptr(setup_td_idx);
    let status_td = ohci_td_ptr(status_td_idx);

    // Setup TD: SETUP, DATA0, 8 bytes.
    let setup_phys = setup_pkt.as_ptr() as u32;
    let data_td_phys = data_td_idx.map(|i| ohci_td_phys(i));
    let status_td_phys = ohci_td_phys(status_td_idx);

    let next_after_setup = if data_len > 0 {
        data_td_phys.unwrap()
    } else {
        status_td_phys
    };

    (*setup_td).control = TD_CC_NOT_ACCESSED | TD_T_DATA0 | TD_DI_NO_INTR | TD_DP_SETUP | TD_R_3;
    (*setup_td).cbp = setup_phys;
    (*setup_td).next_td = next_after_setup;
    (*setup_td).be = setup_phys + 7;

    // Data TD(s) — merge all data into one TD (OHCI has no per-TD limit).
    if data_len > 0 {
        let dt_idx = data_td_idx.unwrap();
        let dt = ohci_td_ptr(dt_idx);
        let buf_phys = if let Some(ref mut db) = data {
            db.as_mut_ptr() as u32
        } else {
            0
        };

        let dp = if data_in { TD_DP_IN } else { TD_DP_OUT };
        (*dt).control = TD_CC_NOT_ACCESSED | TD_T_DATA1 | TD_DI_NO_INTR | dp | TD_R_3;
        (*dt).cbp = buf_phys;
        (*dt).next_td = status_td_phys;
        (*dt).be = buf_phys + data_len - 1;
    }

    // Status TD: opposite direction, DATA1, 0 bytes.
    let status_dp = if data_len > 0 && data_in {
        TD_DP_OUT
    } else {
        TD_DP_IN
    };
    (*status_td).control = TD_CC_NOT_ACCESSED | TD_T_DATA1 | TD_DI_NO_INTR | status_dp | TD_R_3;
    (*status_td).cbp = 0;
    (*status_td).next_td = TD_TERMINATE;
    (*status_td).be = 0;

    // Configure the ED.
    let speed_bits = if speed != 0 {
        ED_SPEED_LOW
    } else {
        ED_SPEED_FULL
    };
    let mps = max_pkt.min(255) << ED_MPS_SHIFT; // OHCI MPS is 10 bits
    let fa = (dev_addr as u32) << ED_FA_SHIFT;
    (*ed).control = fa | speed_bits | mps;
    (*ed).tail_td = status_td_phys; // last TD (stop when head reaches this)
    (*ed).head_td = ohci_td_phys(setup_td_idx);
    (*ed).next_ed = ED_TERMINATE; // single ED in control list

    // Link ED into control list.
    ohci_wr(OHCI_HC_CONTROL_HEAD_ED, ohci_ed_phys(ed_idx));
    ohci_wr(OHCI_HC_CONTROL_CURRENT_ED, ohci_ed_phys(ed_idx));

    // Ensure control list is enabled.
    ohci_wr(OHCI_HC_CONTROL, ohci_rd(OHCI_HC_CONTROL) | OHCI_CTRL_CLE);

    // Doorbell: set Control List Filled.
    ohci_wr(OHCI_HC_COMMAND_STATUS, OHCI_CMD_CLF);

    // Poll for completion.
    let mut timeout = 500_000u32;
    loop {
        // Check if controller is still operational.
        let ctrl = ohci_rd(OHCI_HC_CONTROL);
        if (ctrl & OHCI_CTRL_HCFS_MASK) != OHCI_CTRL_HCFS_OPER {
            ohci_wr(OHCI_HC_CONTROL_HEAD_ED, 0);
            ohci_wr(OHCI_HC_CONTROL_CURRENT_ED, 0);
            return Err(Errno::Io);
        }

        // Check if ED is halted (head_td bit 0 set by HC).
        if (*ed).head_td & 1 != 0 {
            ohci_wr(OHCI_HC_CONTROL_HEAD_ED, 0);
            ohci_wr(OHCI_HC_CONTROL_CURRENT_ED, 0);
            return Err(Errno::Io);
        }

        // Check if all TDs are processed: head == tail.
        let head_p = (*ed).head_td & !1;
        let tail_p = (*ed).tail_td & !0xF;
        if head_p == tail_p {
            // All TDs processed. Verify condition codes.
            let setup_cc = (*setup_td).control & TD_CC_MASK;
            if setup_cc != 0 {
                ohci_wr(OHCI_HC_CONTROL_HEAD_ED, 0);
                ohci_wr(OHCI_HC_CONTROL_CURRENT_ED, 0);
                return Err(Errno::Io);
            }
            if data_len > 0 {
                let dt = ohci_td_ptr(data_td_idx.unwrap());
                let data_cc = (*dt).control & TD_CC_MASK;
                if data_cc != 0 {
                    ohci_wr(OHCI_HC_CONTROL_HEAD_ED, 0);
                    ohci_wr(OHCI_HC_CONTROL_CURRENT_ED, 0);
                    return Err(Errno::Io);
                }
            }
            let status_cc = (*status_td).control & TD_CC_MASK;
            if status_cc != 0 {
                ohci_wr(OHCI_HC_CONTROL_HEAD_ED, 0);
                ohci_wr(OHCI_HC_CONTROL_CURRENT_ED, 0);
                return Err(Errno::Io);
            }

            // Success — clear control list pointers.
            ohci_wr(OHCI_HC_CONTROL_HEAD_ED, 0);
            ohci_wr(OHCI_HC_CONTROL_CURRENT_ED, 0);
            return Ok(data_len);
        }

        if timeout == 0 {
            ohci_wr(OHCI_HC_CONTROL_HEAD_ED, 0);
            ohci_wr(OHCI_HC_CONTROL_CURRENT_ED, 0);
            return Err(Errno::Io);
        }
        timeout -= 1;
    }
}

/// Submit an OHCI bulk transfer (data-only, no setup/status).
pub unsafe fn ohci_bulk_transfer(
    dev_addr: u8,
    mut data: Option<&mut [u8]>,
    data_in: bool,
    max_pkt: u32,
    speed: u8,
) -> KResult<u32> {
    let data_len = data.as_ref().map(|d| d.len() as u32).unwrap_or(0);
    if data_len == 0 {
        return Ok(0);
    }

    if 1 > MAX_OHCI_TD as u32 {
        return Err(Errno::NoMem);
    }

    let ed_idx = ohci_alloc_ed()?;
    let data_td_idx = ohci_alloc_td()?;

    let ed = ohci_ed_ptr(ed_idx);
    let dt = ohci_td_ptr(data_td_idx);

    let buf_phys = if let Some(ref mut db) = data {
        db.as_mut_ptr() as u32
    } else {
        0
    };
    let dp = if data_in { TD_DP_IN } else { TD_DP_OUT };
    (*dt).control = TD_CC_NOT_ACCESSED | TD_T_DATA0 | TD_DI_NO_INTR | dp | TD_R_3;
    (*dt).cbp = buf_phys;
    (*dt).next_td = TD_TERMINATE;
    (*dt).be = buf_phys + data_len - 1;

    let speed_bits = if speed != 0 {
        ED_SPEED_LOW
    } else {
        ED_SPEED_FULL
    };
    let mps = max_pkt.min(255) << ED_MPS_SHIFT;
    let fa = (dev_addr as u32) << ED_FA_SHIFT;
    (*ed).control = fa | speed_bits | mps;
    (*ed).tail_td = TD_TERMINATE;
    (*ed).head_td = ohci_td_phys(data_td_idx);
    (*ed).next_ed = ED_TERMINATE;

    ohci_wr(OHCI_HC_BULK_HEAD_ED, ohci_ed_phys(ed_idx));
    ohci_wr(OHCI_HC_BULK_CURRENT_ED, ohci_ed_phys(ed_idx));

    ohci_wr(OHCI_HC_CONTROL, ohci_rd(OHCI_HC_CONTROL) | OHCI_CTRL_BLE);

    ohci_wr(OHCI_HC_COMMAND_STATUS, OHCI_CMD_CLF);

    let mut timeout = 500_000u32;
    loop {
        let ctrl = ohci_rd(OHCI_HC_CONTROL);
        if (ctrl & OHCI_CTRL_HCFS_MASK) != OHCI_CTRL_HCFS_OPER {
            ohci_wr(OHCI_HC_BULK_HEAD_ED, 0);
            ohci_wr(OHCI_HC_BULK_CURRENT_ED, 0);
            return Err(Errno::Io);
        }

        if (*ed).head_td & 1 != 0 {
            ohci_wr(OHCI_HC_BULK_HEAD_ED, 0);
            ohci_wr(OHCI_HC_BULK_CURRENT_ED, 0);
            return Err(Errno::Io);
        }

        let head_p = (*ed).head_td & !1;
        let tail_p = (*ed).tail_td & !0xF;
        if head_p == tail_p {
            let data_cc = (*dt).control & TD_CC_MASK;
            if data_cc != 0 {
                ohci_wr(OHCI_HC_BULK_HEAD_ED, 0);
                ohci_wr(OHCI_HC_BULK_CURRENT_ED, 0);
                return Err(Errno::Io);
            }
            ohci_wr(OHCI_HC_BULK_HEAD_ED, 0);
            ohci_wr(OHCI_HC_BULK_CURRENT_ED, 0);
            return Ok(data_len);
        }

        if timeout == 0 {
            ohci_wr(OHCI_HC_BULK_HEAD_ED, 0);
            ohci_wr(OHCI_HC_BULK_CURRENT_ED, 0);
            return Err(Errno::Io);
        }
        timeout -= 1;
    }
}

/// Number of OHCI root-hub ports.
pub unsafe fn ohci_n_ports() -> u8 {
    G_OHCI_N_PORTS
}

/// Read OHCI root-hub port status.
pub unsafe fn ohci_port_status(idx: u8) -> KResult<u32> {
    if idx >= G_OHCI_N_PORTS {
        return Err(Errno::Range);
    }
    let reg = OHCI_HC_RH_PORT_STATUS + 4 * idx as u32;
    Ok(ohci_rd(reg))
}

/// Reset an OHCI root-hub port (set PRS, wait for self-clear).
pub unsafe fn ohci_port_reset(idx: u8) -> KResult<()> {
    if idx >= G_OHCI_N_PORTS {
        return Err(Errno::Range);
    }
    let reg = OHCI_HC_RH_PORT_STATUS + 4 * idx as u32;
    // Ensure port power is on.
    ohci_wr(reg, ohci_rd(reg) | RH_PS_PPS);
    // Set port reset.
    ohci_wr(reg, ohci_rd(reg) | RH_PS_PRS);
    // Wait for PRS to self-clear.
    let mut timeout = 100_000u32;
    while timeout > 0 && (ohci_rd(reg) & RH_PS_PRS) != 0 {
        timeout -= 1;
    }
    if timeout == 0 {
        return Err(Errno::Io);
    }
    // Enable the port.
    ohci_wr(reg, ohci_rd(reg) | RH_PS_PES);
    Ok(())
}

/// Enable an OHCI root-hub port.
pub unsafe fn ohci_port_enable(idx: u8) -> KResult<()> {
    if idx >= G_OHCI_N_PORTS {
        return Err(Errno::Range);
    }
    let reg = OHCI_HC_RH_PORT_STATUS + 4 * idx as u32;
    ohci_wr(reg, ohci_rd(reg) | RH_PS_PES);
    Ok(())
}

/// Get the speed of a device attached to an OHCI port (0 = Full, 1 = Low).
pub unsafe fn ohci_port_speed(idx: u8) -> KResult<u8> {
    let ps = ohci_port_status(idx)?;
    Ok(if (ps & RH_PS_LSDA) != 0 { 1 } else { 0 })
}

/// Submit a control transfer on the active controller.
pub unsafe fn control_transfer(
    dev_addr: u8,
    setup_pkt: &[u8; 8],
    data: Option<&mut [u8]>,
    data_in: bool,
    max_pkt: u32,
) -> KResult<u32> {
    match G_ACTIVE {
        ControllerType::Ehci => ehci_control_transfer(dev_addr, setup_pkt, data, data_in, max_pkt),
        ControllerType::Ohci => {
            ohci_control_transfer(dev_addr, setup_pkt, data, data_in, max_pkt, 0)
        }
        ControllerType::None => Err(Errno::NoSys),
    }
}

/// Number of root-hub ports on the active controller.
pub fn n_ports() -> u8 {
    unsafe {
        match G_ACTIVE {
            ControllerType::Ehci => G_N_PORTS,
            ControllerType::Ohci => G_OHCI_N_PORTS,
            ControllerType::None => 0,
        }
    }
}

/// Read the status of root-hub port `idx` (0-based).
pub unsafe fn port_status(idx: u8) -> KResult<u32> {
    match G_ACTIVE {
        ControllerType::Ehci => {
            if idx >= G_N_PORTS {
                return Err(Errno::Range);
            }
            Ok(op_rd(OP_PORTSC + 4 * idx as u32))
        }
        ControllerType::Ohci => ohci_port_status(idx),
        ControllerType::None => Err(Errno::NoSys),
    }
}

/// Reset a root-hub port.
pub unsafe fn port_reset(idx: u8) -> KResult<()> {
    match G_ACTIVE {
        ControllerType::Ehci => {
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
        ControllerType::Ohci => ohci_port_reset(idx),
        ControllerType::None => Err(Errno::NoSys),
    }
}

/// Enable a root-hub port.
pub unsafe fn port_enable(idx: u8) -> KResult<()> {
    match G_ACTIVE {
        ControllerType::Ehci => {
            if idx >= G_N_PORTS {
                return Err(Errno::Range);
            }
            let reg = OP_PORTSC + 4 * idx as u32;
            op_wr(reg, op_rd(reg) | (1 << 2));
            Ok(())
        }
        ControllerType::Ohci => ohci_port_enable(idx),
        ControllerType::None => Err(Errno::NoSys),
    }
}

/// Probe for and initialise the first available USB controller (EHCI or OHCI).
pub unsafe fn init_usb() -> KResult<()> {
    if probe_ehci(EHCI_BASE) {
        init_ehci(EHCI_BASE)
    } else if probe_ohci(OHCI_BASE) {
        init_ohci(OHCI_BASE)
    } else {
        Err(Errno::NoEnt)
    }
}

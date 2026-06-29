//! USB host controller — EHCI async schedule with control/bulk transfers.
//!
//! Provides a minimal async list scheduler for EHCI, supporting synchronous
//! control and bulk transfers. Used by hub drivers for device enumeration.
//! The periodic schedule (interrupt/isochronous) is not implemented.
use crate::arch::mmio::Mmio;
use onyx_core::errno::{Errno, KResult};

pub const EHCI_BASE: usize = 0x04C0_0000;
pub const OHCI_BASE: usize = 0x04C1_0000;

// OHCI register offsets (for probe only — no OHCI driver implementation).
const OHCI_HC_REV: u32 = 0x00;

// EHCI capability register offsets.
const EHCI_CAP_LENGTH: u32 = 0x00;
const EHCI_CAP_VERSION: u32 = 0x02;
const EHCI_CAP_HCSPARAMS: u32 = 0x04;
const EHCI_CAP_HCCPARAMS: u32 = 0x08;

// EHCI operational register offsets (relative to op_base).
const OP_USBCMD: u32 = 0x00;
const OP_USBSTS: u32 = 0x04;
const OP_USBINTR: u32 = 0x08;
const OP_FRINDEX: u32 = 0x0C;
const OP_CTRLDSSEGMENT: u32 = 0x10;
const OP_PERIODICLISTBASE: u32 = 0x14;
const OP_ASYNCLISTADDR: u32 = 0x18;
const OP_CONFIGFLAG: u32 = 0x40;
const OP_PORTSC: u32 = 0x44;

// USBCMD bits.
const CMD_RUN: u32 = 1 << 0;
const CMD_RESET: u32 = 1 << 1;
const CMD_ASYNC_ENABLE: u32 = 1 << 5;
const CMD_PERIODIC_ENABLE: u32 = 1 << 4;

// USBSTS bits.
const STS_HCHALTED: u32 = 1 << 12;
const STS_ASYNC_ADVANCE: u32 = 1 << 5;

// qTD token bits.
const QTD_ACTIVE: u32 = 1 << 7;
const QTD_HALTED: u32 = 1 << 6;
const QTD_BUF_ERR: u32 = 1 << 5;
const QTD_BABBLE: u32 = 1 << 4;
const QTD_XACT_ERR: u32 = 1 << 3;
const QTD_MISSED_MF: u32 = 1 << 2;
const QTD_ERROR: u32 = QTD_HALTED | QTD_BUF_ERR | QTD_BABBLE | QTD_XACT_ERR;
const QTD_PID_OUT: u32 = 0 << 8;
const QTD_PID_IN: u32 = 1 << 8;
const QTD_PID_SETUP: u32 = 2 << 8;
const QTD_CERR_MASK: u32 = 3 << 10;
const QTD_CERR_3: u32 = 3 << 10;
const QTD_TOGGLE: u32 = 1 << 14;
const QTD_TOTAL_LEN_SHIFT: u32 = 16;

// QH endpoint chars bits.
const QH_DEV_ADDR_SHIFT: u32 = 8;
const QH_DEV_ADDR_MASK: u32 = 0x7F << 8;
const QH_INACTIVATE: u32 = 1 << 7;
const QH_EPS_SHIFT: u32 = 12;
const QH_EPS_HIGH: u32 = 2 << QH_EPS_SHIFT;
const QH_EPS_FULL: u32 = 0 << QH_EPS_SHIFT;
const QH_EPS_LOW: u32 = 1 << QH_EPS_SHIFT;
const QH_DTC: u32 = 1 << 14;
const QH_HRL: u32 = 1 << 15;
const QH_MPL_SHIFT: u32 = 16;
const QH_MPL_MASK: u32 = 0x7FF << QH_MPL_SHIFT;
const QH_C: u32 = 1 << 0;
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
static mut G_DMA: DmaPool = DmaPool { data: [0; DMA_POOL_SIZE] };
static mut G_DMA_USED: usize = 0;

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
const QH_SIZE: usize = core::mem::size_of::<QH>();

#[repr(C)]
struct QTD {
    next: u32,
    alt_next: u32,
    token: u32,
    buf: [u32; 5],
}
const QTD_SIZE: usize = core::mem::size_of::<QTD>();

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
    core::ptr::write_bytes(G_DMA.data.as_mut_ptr().add(off), 0, aligned);
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
pub unsafe fn control_transfer(
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
    let next_phys = if qtd_count > 1 { qtd_phys(qtd_indices[1]) } else { QH_TERMINATE };
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
        } else { 0 };
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
    let status_pid = if data_in || data_len == 0 { QTD_PID_OUT } else { QTD_PID_IN };
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

    Ok(())
}

/// Initialise OHCI (stub — not yet implemented).
pub unsafe fn init_ohci(base: usize) -> KResult<()> {
    if !probe_ohci(base) {
        return Err(Errno::NoEnt);
    }
    Err(Errno::NoSys)
}

/// Number of root-hub ports on the active controller.
pub fn n_ports() -> u8 {
    unsafe { G_N_PORTS }
}

/// Read the status of root-hub port `idx` (0-based).
pub unsafe fn port_status(idx: u8) -> KResult<u32> {
    if idx >= G_N_PORTS {
        return Err(Errno::Range);
    }
    Ok(op_rd(OP_PORTSC + 4 * idx as u32))
}

/// Reset a root-hub port.
pub unsafe fn port_reset(idx: u8) -> KResult<()> {
    if idx >= G_N_PORTS {
        return Err(Errno::Range);
    }
    let reg = OP_PORTSC + 4 * idx as u32;
    // Set reset bit (bit 8).
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

/// Enable a root-hub port (port enable bit, bit 2).
pub unsafe fn port_enable(idx: u8) -> KResult<()> {
    if idx >= G_N_PORTS {
        return Err(Errno::Range);
    }
    let reg = OP_PORTSC + 4 * idx as u32;
    op_wr(reg, op_rd(reg) | (1 << 2));
    Ok(())
}

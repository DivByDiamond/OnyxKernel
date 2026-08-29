use crate::arch::mmio::Mmio;
use onyx_core::errno::{Errno, KResult};

use super::{
    CMD_ASYNC_ENABLE, CMD_RESET, EHCI_CAP_HCSPARAMS, G_ASYNCLIST_ENABLED, G_N_PORTS, G_OP_BASE,
    OP_ASYNCLISTADDR, OP_CONFIGFLAG, OP_USBCMD, OP_USBSTS, QH, QH_HRL, QH_INACTIVATE, QH_QH,
    QH_TERMINATE, STS_ASYNC_ADVANCE, STS_HCHALTED, alloc_qh, op_rd, op_wr, qh_phys, qh_ptr,
};

/// # Safety
///
/// Must be called after `init_ehci` has set `G_OP_BASE`, from the driver's
/// single-threaded USB path with SIE=0 (see `crate::sync`).
pub(super) unsafe fn init_async_list() -> KResult<()> {
    // SAFETY: runs after init_ehci with SIE=0 (see crate::sync); the head QH comes from alloc_qh so every raw-pointer write below lands inside the G_DMA pool; op_rd/op_wr target in-file register offsets.
    unsafe {
        if G_ASYNCLIST_ENABLED {
            return Ok(());
        }
        let head_idx = alloc_qh()?;
        let head = qh_ptr(head_idx);
        let head_phys = qh_phys(head_idx);
        (*head).horz_link = head_phys | QH_QH;
        (*head).ep_chars = QH_HRL | QH_INACTIVATE;
        (*head).eps_bits = 0;
        (*head).current_link = 0;
        (*head).overlay_next = QH_TERMINATE;
        (*head).overlay_alt_next = QH_TERMINATE;
        (*head).overlay_token = 0;
        op_wr(OP_ASYNCLISTADDR, head_phys);
        op_wr(OP_USBCMD, op_rd(OP_USBCMD) | CMD_ASYNC_ENABLE);
        let mut timeout = 1000u32;
        while timeout > 0 && (op_rd(OP_USBSTS) & STS_HCHALTED) != 0 {
            timeout -= 1;
        }
        G_ASYNCLIST_ENABLED = true;
        Ok(())
    }
}

/// # Safety
///
/// `idx` must be a QH index from `alloc_qh`, called after `init_ehci` from
/// the driver's single-threaded USB path with SIE=0.
pub(super) unsafe fn qh_insert(idx: usize) {
    // SAFETY: qh_ptr(idx) is inside the G_DMA pool (idx from alloc_qh); head_phys read back from OP_ASYNCLISTADDR was previously programmed from qh_phys, i.e. an in-pool address; MMIO offsets are in-file constants.
    unsafe {
        if !G_ASYNCLIST_ENABLED {
            return;
        }
        let head_phys = op_rd(OP_ASYNCLISTADDR) & !0x1F;
        let qh = qh_ptr(idx);
        let qh_phys_addr = super::qh_phys(idx);
        (*qh).horz_link = head_phys | QH_QH;
        let head = (head_phys as usize) as *mut QH;
        (*head).horz_link = qh_phys_addr | QH_QH;
        if (op_rd(OP_USBSTS) & STS_ASYNC_ADVANCE) != 0 {
            op_wr(OP_USBSTS, STS_ASYNC_ADVANCE);
        }
    }
}

/// # Safety
///
/// Same contract as `qh_insert`: `idx` from `alloc_qh`, async list
/// initialized, single-threaded USB path with SIE=0.
pub(super) unsafe fn qh_remove(idx: usize) {
    // SAFETY: the list walk only follows physical addresses previously written from qh_phys (all inside the G_DMA pool) and qh_ptr(idx) is in-pool; MMIO offsets are in-file constants; SIE=0 per crate::sync.
    unsafe {
        if !G_ASYNCLIST_ENABLED {
            return;
        }
        let head_phys = op_rd(OP_ASYNCLISTADDR) & !0x1F;
        let qh_phys_addr = super::qh_phys(idx);
        let mut prev_phys = head_phys;
        loop {
            let prev = (prev_phys as usize) as *const QH;
            let next = (*prev).horz_link & !0x1F;
            if next == qh_phys_addr {
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
}

/// # Safety
///
/// Must run once on the boot hart during single-threaded early boot before
/// secondary harts start, with `base` equal to the controller's MMIO base
/// (identity-mapped at boot and validated by `probe_ehci`).
pub unsafe fn init_ehci(base: usize) -> KResult<()> {
    // SAFETY: base is the platform constant passed by init_usb after probe_ehci validated it, identity-mapped at boot; G_OP_BASE/G_N_PORTS/G_ACTIVE are written here during single-threaded init before secondary harts start; register offsets are in-file constants.
    unsafe {
        let cap_len = Mmio::<u32>::at(base).read() & 0xFF;
        G_OP_BASE = base + cap_len as usize;
        G_N_PORTS = ((Mmio::<u32>::at(base + EHCI_CAP_HCSPARAMS as usize)).read() >> 24) as u8;
        crate::drivers::usb::G_ACTIVE = crate::drivers::usb::ControllerType::Ehci;
        op_wr(OP_USBCMD, op_rd(OP_USBCMD) | CMD_RESET);
        let mut timeout = 1000u32;
        while timeout > 0 && (op_rd(OP_USBCMD) & CMD_RESET) != 0 {
            timeout -= 1;
        }
        if timeout == 0 {
            return Err(Errno::Io);
        }
        op_wr(OP_CONFIGFLAG, 1);
        init_async_list()
    }
}

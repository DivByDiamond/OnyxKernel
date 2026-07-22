use crate::arch::mmio::Mmio;
use onyx_core::errno::{Errno, KResult};

use super::{
    alloc_qh, op_rd, op_wr, qh_phys, qh_ptr, CMD_ASYNC_ENABLE, CMD_RESET, EHCI_CAP_HCSPARAMS,
    G_ASYNCLIST_ENABLED, G_N_PORTS, G_OP_BASE, OP_ASYNCLISTADDR, OP_CONFIGFLAG, OP_USBCMD,
    OP_USBSTS, QH, QH_HRL, QH_INACTIVATE, QH_QH, QH_TERMINATE, STS_ASYNC_ADVANCE, STS_HCHALTED,
};

pub(super) unsafe fn init_async_list() -> KResult<()> {
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

pub(super) unsafe fn qh_insert(idx: usize) {
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

pub(super) unsafe fn qh_remove(idx: usize) {
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

pub(super) unsafe fn init_ehci(base: usize) -> KResult<()> {
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

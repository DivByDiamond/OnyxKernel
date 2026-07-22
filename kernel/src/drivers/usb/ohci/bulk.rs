use crate::mm::pmm;
use onyx_core::errno::{Errno, KResult};

use super::{
    ohci_alloc_ed, ohci_alloc_td, ohci_ed_phys, ohci_ed_ptr, ohci_rd, ohci_td_phys, ohci_td_ptr,
    ohci_wr, ED_FA_SHIFT, ED_MPS_SHIFT, ED_SPEED_FULL, ED_SPEED_LOW, ED_TERMINATE, G_OHCI_BASE,
    G_OHCI_HCCA_PA, G_OHCI_HCCA_READY, G_OHCI_N_PORTS, OHCI_CMD_CLF, OHCI_CMD_HCR, OHCI_CTRL_BLE,
    OHCI_CTRL_CLE, OHCI_CTRL_HCFS_MASK, OHCI_CTRL_HCFS_OPER, OHCI_CTRL_RWE,
    OHCI_HC_BULK_CURRENT_ED, OHCI_HC_BULK_HEAD_ED, OHCI_HC_COMMAND_STATUS, OHCI_HC_CONTROL,
    OHCI_HC_CONTROL_CURRENT_ED, OHCI_HC_CONTROL_HEAD_ED, OHCI_HC_FM_INTERVAL, OHCI_HC_HCCA,
    OHCI_HC_INTERRUPT_STATUS, OHCI_HC_LS_THRESHOLD, OHCI_HC_PERIODIC_START, OHCI_HC_RH_DESC_A,
    OHCI_HC_RH_PORT_STATUS, OHCI_HC_RH_STATUS, RH_PS_CSC, RH_PS_PES, RH_PS_PPS, RH_PS_PRS,
    RH_PS_PRSC, TD_CC_MASK, TD_CC_NOT_ACCESSED, TD_DI_NO_INTR, TD_DP_IN, TD_DP_OUT, TD_R_3,
    TD_TERMINATE, TD_T_DATA0,
};

pub(super) unsafe fn ohci_bulk_transfer(
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

pub(super) unsafe fn init_ohci(base: usize) -> KResult<()> {
    if !super::probe_ohci(base) {
        return Err(Errno::NoEnt);
    }

    G_OHCI_BASE = base;

    let desc_a = ohci_rd(OHCI_HC_RH_DESC_A);
    G_OHCI_N_PORTS = (desc_a & 0xFF) as u8;

    let hcca_pa = pmm::alloc_zero()? as u32;
    G_OHCI_HCCA_PA = hcca_pa;
    G_OHCI_HCCA_READY = true;

    ohci_wr(OHCI_HC_HCCA, hcca_pa);

    ohci_wr(OHCI_HC_COMMAND_STATUS, OHCI_CMD_HCR);
    let mut timeout = 100_000u32;
    while timeout > 0 && (ohci_rd(OHCI_HC_COMMAND_STATUS) & OHCI_CMD_HCR) != 0 {
        timeout -= 1;
    }
    if timeout == 0 {
        G_OHCI_HCCA_READY = false;
        return Err(Errno::Io);
    }

    let fi = ohci_rd(OHCI_HC_FM_INTERVAL);
    let fi_fit = fi & 0x3FFF;
    let fsmps = ((fi_fit * 9) / 10) << 16;
    ohci_wr(OHCI_HC_FM_INTERVAL, (fi & 0xFFFF) | fsmps);

    let periodic_start = (fi_fit * 9) / 10;
    ohci_wr(OHCI_HC_PERIODIC_START, periodic_start);
    ohci_wr(OHCI_HC_LS_THRESHOLD, 0x628);

    ohci_wr(OHCI_HC_CONTROL_HEAD_ED, 0);
    ohci_wr(OHCI_HC_CONTROL_CURRENT_ED, 0);
    ohci_wr(OHCI_HC_BULK_HEAD_ED, 0);
    ohci_wr(OHCI_HC_BULK_CURRENT_ED, 0);

    ohci_wr(OHCI_HC_INTERRUPT_STATUS, 0xFFFF_FFFF);

    ohci_wr(
        OHCI_HC_CONTROL,
        OHCI_CTRL_HCFS_OPER | OHCI_CTRL_CLE | OHCI_CTRL_BLE | OHCI_CTRL_RWE,
    );

    timeout = 100_000u32;
    while timeout > 0 && (ohci_rd(OHCI_HC_CONTROL) & OHCI_CTRL_HCFS_MASK) != OHCI_CTRL_HCFS_OPER {
        timeout -= 1;
    }
    if timeout == 0 {
        G_OHCI_HCCA_READY = false;
        return Err(Errno::Io);
    }

    ohci_wr(OHCI_HC_RH_STATUS, 1 << 16);

    for i in 0..G_OHCI_N_PORTS {
        let port_reg = OHCI_HC_RH_PORT_STATUS + 4 * i as u32;
        let ps = ohci_rd(port_reg);
        ohci_wr(port_reg, ps | RH_PS_PPS);
    }

    for i in 0..G_OHCI_N_PORTS {
        let port_reg = OHCI_HC_RH_PORT_STATUS + 4 * i as u32;
        ohci_wr(port_reg, RH_PS_CSC | RH_PS_PRSC);
    }

    crate::drivers::usb::G_ACTIVE = crate::drivers::usb::ControllerType::Ohci;
    Ok(())
}

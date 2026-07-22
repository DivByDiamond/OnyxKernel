use crate::arch::mmio::Mmio;
use onyx_core::errno::{Errno, KResult};

use super::{
    ohci_alloc_ed, ohci_alloc_td, ohci_ed_phys, ohci_ed_ptr, ohci_rd, ohci_td_phys, ohci_td_ptr,
    ohci_wr, OhciED, OhciTD, ED_FA_SHIFT, ED_MPS_SHIFT, ED_SPEED_FULL, ED_SPEED_LOW, ED_TERMINATE,
    G_OHCI_N_PORTS, MAX_OHCI_TD, OHCI_CMD_CLF, OHCI_CTRL_CLE, OHCI_CTRL_HCFS_MASK,
    OHCI_CTRL_HCFS_OPER, OHCI_HC_COMMAND_STATUS, OHCI_HC_CONTROL, OHCI_HC_CONTROL_CURRENT_ED,
    OHCI_HC_CONTROL_HEAD_ED, OHCI_HC_REV, OHCI_HC_RH_PORT_STATUS, RH_PS_LSDA, RH_PS_PES, RH_PS_PPS,
    RH_PS_PRS, TD_CC_MASK, TD_CC_NOT_ACCESSED, TD_DI_NO_INTR, TD_DP_IN, TD_DP_OUT, TD_DP_SETUP,
    TD_R_3, TD_TERMINATE, TD_T_DATA0, TD_T_DATA1,
};

pub(super) unsafe fn ohci_control_transfer(
    dev_addr: u8,
    setup_pkt: &[u8; 8],
    mut data: Option<&mut [u8]>,
    data_in: bool,
    max_pkt: u32,
    speed: u8,
) -> KResult<u32> {
    let data_len = data.as_ref().map(|d| d.len() as u32).unwrap_or(0);

    if 2 + if data_len > 0 { 1 } else { 0 } > MAX_OHCI_TD as u32 {
        return Err(Errno::NoMem);
    }

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

    let status_dp = if data_len > 0 && data_in {
        TD_DP_OUT
    } else {
        TD_DP_IN
    };
    (*status_td).control = TD_CC_NOT_ACCESSED | TD_T_DATA1 | TD_DI_NO_INTR | status_dp | TD_R_3;
    (*status_td).cbp = 0;
    (*status_td).next_td = TD_TERMINATE;
    (*status_td).be = 0;

    let speed_bits = if speed != 0 {
        ED_SPEED_LOW
    } else {
        ED_SPEED_FULL
    };
    let mps = max_pkt.min(255) << ED_MPS_SHIFT;
    let fa = (dev_addr as u32) << ED_FA_SHIFT;
    (*ed).control = fa | speed_bits | mps;
    (*ed).tail_td = status_td_phys;
    (*ed).head_td = ohci_td_phys(setup_td_idx);
    (*ed).next_ed = ED_TERMINATE;

    ohci_wr(OHCI_HC_CONTROL_HEAD_ED, ohci_ed_phys(ed_idx));
    ohci_wr(OHCI_HC_CONTROL_CURRENT_ED, ohci_ed_phys(ed_idx));
    ohci_wr(OHCI_HC_CONTROL, ohci_rd(OHCI_HC_CONTROL) | OHCI_CTRL_CLE);
    ohci_wr(OHCI_HC_COMMAND_STATUS, OHCI_CMD_CLF);

    let mut timeout = 500_000u32;
    loop {
        let ctrl = ohci_rd(OHCI_HC_CONTROL);
        if (ctrl & OHCI_CTRL_HCFS_MASK) != OHCI_CTRL_HCFS_OPER {
            ohci_wr(OHCI_HC_CONTROL_HEAD_ED, 0);
            ohci_wr(OHCI_HC_CONTROL_CURRENT_ED, 0);
            return Err(Errno::Io);
        }

        if (*ed).head_td & 1 != 0 {
            ohci_wr(OHCI_HC_CONTROL_HEAD_ED, 0);
            ohci_wr(OHCI_HC_CONTROL_CURRENT_ED, 0);
            return Err(Errno::Io);
        }

        let head_p = (*ed).head_td & !1;
        let tail_p = (*ed).tail_td & !0xF;
        if head_p == tail_p {
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

pub(super) unsafe fn probe_ohci(base: usize) -> bool {
    if base == 0 {
        return false;
    }
    let v = Mmio::<u32>::at(base + OHCI_HC_REV as usize).read();
    (v & 0xFF) == 0 && ((v >> 16) & 0xFFFF) >= 0x10
}

pub(super) unsafe fn ohci_port_status(idx: u8) -> KResult<u32> {
    if idx >= G_OHCI_N_PORTS {
        return Err(Errno::Range);
    }
    let reg = OHCI_HC_RH_PORT_STATUS + 4 * idx as u32;
    Ok(ohci_rd(reg))
}

pub(super) unsafe fn ohci_port_reset(idx: u8) -> KResult<()> {
    if idx >= G_OHCI_N_PORTS {
        return Err(Errno::Range);
    }
    let reg = OHCI_HC_RH_PORT_STATUS + 4 * idx as u32;
    ohci_wr(reg, ohci_rd(reg) | RH_PS_PPS);
    ohci_wr(reg, ohci_rd(reg) | RH_PS_PRS);
    let mut timeout = 100_000u32;
    while timeout > 0 && (ohci_rd(reg) & RH_PS_PRS) != 0 {
        timeout -= 1;
    }
    if timeout == 0 {
        return Err(Errno::Io);
    }
    ohci_wr(reg, ohci_rd(reg) | RH_PS_PES);
    Ok(())
}

pub(super) unsafe fn ohci_port_enable(idx: u8) -> KResult<()> {
    if idx >= G_OHCI_N_PORTS {
        return Err(Errno::Range);
    }
    let reg = OHCI_HC_RH_PORT_STATUS + 4 * idx as u32;
    ohci_wr(reg, ohci_rd(reg) | RH_PS_PES);
    Ok(())
}

pub(super) unsafe fn ohci_port_speed(idx: u8) -> KResult<u8> {
    let ps = ohci_port_status(idx)?;
    Ok(if (ps & RH_PS_LSDA) != 0 { 1 } else { 0 })
}

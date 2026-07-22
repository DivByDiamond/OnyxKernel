use onyx_core::errno::{Errno, KResult};

use super::{
    alloc_qh, alloc_qtd, op_rd, op_wr, qh_phys, qh_ptr, qtd_phys, qtd_ptr, G_ASYNCLIST_ENABLED,
    MAX_QTD, OP_USBCMD, OP_USBSTS, QH, QH_DEV_ADDR_SHIFT, QH_DTC, QH_EPS_HIGH, QH_MPL_SHIFT,
    QH_TERMINATE, QTD, QTD_ACTIVE, QTD_BUF_SIZE, QTD_CERR_3, QTD_ERROR, QTD_PID_IN, QTD_PID_OUT,
    QTD_PID_SETUP, QTD_TOGGLE, QTD_TOTAL_LEN_SHIFT, STS_HCHALTED,
};

pub(super) unsafe fn ehci_control_transfer(
    dev_addr: u8,
    setup_pkt: &[u8; 8],
    mut data: Option<&mut [u8]>,
    data_in: bool,
    max_pkt: u32,
) -> KResult<u32> {
    if !G_ASYNCLIST_ENABLED {
        super::queue::init_async_list()?;
    }
    let data_len = data.as_ref().map(|d| d.len() as u32).unwrap_or(0);
    let data_qtds = if data_len > 0 {
        (data_len + QTD_BUF_SIZE - 1) / QTD_BUF_SIZE
    } else {
        0
    };
    let qtd_count = 1 + data_qtds + 1;
    if qtd_count > MAX_QTD as u32 {
        return Err(Errno::NoMem);
    }
    let qh_idx = alloc_qh()?;
    let qh = qh_ptr(qh_idx);
    let setup_phys = setup_pkt.as_ptr() as u32;
    let data_pid = if data_in { QTD_PID_IN } else { QTD_PID_OUT };
    let mut qtd_indices = [0usize; 64];
    for i in 0..qtd_count as usize {
        qtd_indices[i] = alloc_qtd()?;
    }
    let sqtd = qtd_ptr(qtd_indices[0]);
    let next_phys = if qtd_count > 1 {
        qtd_phys(qtd_indices[1])
    } else {
        QH_TERMINATE
    };
    (*sqtd).next = next_phys;
    (*sqtd).alt_next = QH_TERMINATE;
    (*sqtd).token = QTD_PID_SETUP | QTD_CERR_3 | (8 << QTD_TOTAL_LEN_SHIFT) | QTD_ACTIVE;
    (*sqtd).buf = [setup_phys, 0, 0, 0, 0];
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
    let status_pid = if data_in || data_len == 0 {
        QTD_PID_OUT
    } else {
        QTD_PID_IN
    };
    let stqtd = qtd_ptr(qtd_indices[qtd_count as usize - 1]);
    (*stqtd).next = QH_TERMINATE;
    (*stqtd).alt_next = QH_TERMINATE;
    (*stqtd).token = status_pid | QTD_CERR_3 | QTD_TOGGLE | QTD_ACTIVE;
    (*stqtd).buf = [0, 0, 0, 0, 0];
    let mpl_val = max_pkt.min(8) << QH_MPL_SHIFT;
    let dev_addr_bits = (dev_addr as u32) << QH_DEV_ADDR_SHIFT;
    (*qh).horz_link = 0;
    (*qh).ep_chars = dev_addr_bits | QH_EPS_HIGH | QH_DTC | mpl_val;
    (*qh).eps_bits = 0;
    (*qh).current_link = qtd_phys(qtd_indices[0]);
    (*qh).overlay_next = qtd_phys(qtd_indices[0]);
    (*qh).overlay_alt_next = QH_TERMINATE;
    (*qh).overlay_token = QTD_PID_SETUP | QTD_CERR_3 | (8 << QTD_TOTAL_LEN_SHIFT) | QTD_ACTIVE;
    (*qh).overlay_buf = [setup_phys, 0, 0, 0, 0];
    super::queue::qh_insert(qh_idx);
    let mut timeout = 500_000u32;
    let mut bytes_xfered = 0u32;
    loop {
        if (op_rd(OP_USBSTS) & STS_HCHALTED) != 0 {
            super::queue::qh_remove(qh_idx);
            return Err(Errno::Io);
        }
        if ((*sqtd).token & QTD_ACTIVE) == 0 {
            if ((*sqtd).token & QTD_ERROR) != 0 {
                super::queue::qh_remove(qh_idx);
                return Err(Errno::Io);
            }
            let mut all_done = true;
            for i in 0..data_qtds as usize {
                let dt = qtd_ptr(qtd_indices[1 + i]);
                if ((*dt).token & QTD_ACTIVE) != 0 {
                    all_done = false;
                    break;
                }
                if ((*dt).token & QTD_ERROR) != 0 {
                    super::queue::qh_remove(qh_idx);
                    return Err(Errno::Io);
                }
                bytes_xfered = bytes_xfered.max(buf_pos.min(data_len));
            }
            if all_done {
                let stoken = qtd_ptr(qtd_indices[qtd_count as usize - 1]);
                if ((*stoken).token & QTD_ACTIVE) == 0 && ((*stoken).token & QTD_ERROR) == 0 {
                    super::queue::qh_remove(qh_idx);
                    return Ok(bytes_xfered);
                }
            }
        }
        if timeout == 0 {
            super::queue::qh_remove(qh_idx);
            return Err(Errno::Io);
        }
        timeout -= 1;
    }
}

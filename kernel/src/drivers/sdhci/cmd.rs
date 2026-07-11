use super::regs::*;
use super::{clear_interrupts, reset, wait_idle};

pub(super) unsafe fn send_command(
    base: usize,
    cmd_idx: u16,
    arg: u32,
    resp_type: u16,
    has_data: bool,
) -> u32 {
    wait_idle(base);
    clear_interrupts(base);

    let ps = reg_r(base, PRESENT_STATE);
    if ps & PS_CMD_INHIBIT != 0 {
        reset(base, SW_RESET_CMD);
    }

    if has_data {
        reg_w16(base, BLOCK_SIZE, SDHCI_SECTOR_SIZE as u16);
        reg_w16(base, BLOCK_COUNT, 1);
    }

    reg_w(base, ARGUMENT, arg);

    let mut cmd_reg: u16 = (cmd_idx & 0x3F) << 8;
    cmd_reg |= resp_type;
    if has_data {
        cmd_reg |= CMD_DATA_PRESENT;
    }

    if cmd_idx == CMD_GO_IDLE_STATE {
    } else if cmd_idx == CMD_ALL_SEND_CID || cmd_idx == CMD_SEND_CSD {
        cmd_reg |= CMD_CRC_ENABLE;
    } else if cmd_idx == CMD_SEND_IF_COND || cmd_idx == CMD_APP_CMD {
        cmd_reg |= CMD_CRC_ENABLE | CMD_INDEX_ENABLE;
    } else if cmd_idx == CMD_SEND_RELATIVE_ADDR {
        cmd_reg |= CMD_CRC_ENABLE | CMD_INDEX_ENABLE;
    } else if cmd_idx == CMD_SELECT_CARD {
        cmd_reg |= CMD_CRC_ENABLE | CMD_INDEX_ENABLE;
    } else if cmd_idx == CMD_SET_BLOCKLEN {
        cmd_reg |= CMD_CRC_ENABLE | CMD_INDEX_ENABLE;
    } else if cmd_idx == CMD_READ_SINGLE_BLOCK || cmd_idx == CMD_READ_MULTIPLE_BLOCK {
        cmd_reg |= CMD_CRC_ENABLE | CMD_INDEX_ENABLE;
    } else if cmd_idx == CMD_WRITE_SINGLE_BLOCK || cmd_idx == CMD_WRITE_MULTIPLE_BLOCK {
        cmd_reg |= CMD_CRC_ENABLE | CMD_INDEX_ENABLE;
    }

    reg_w16(base, COMMAND, cmd_reg);

    let mut timeout = SDHCI_TIMEOUT;
    while timeout > 0 {
        let istat = reg_r(base, NORMAL_INT_STATUS);
        if istat & INT_ERROR != 0 {
            clear_interrupts(base);
            reset(base, SW_RESET_CMD);
            return 0xFFFF_FFFF;
        }
        if istat & INT_CMD_COMPLETE != 0 {
            reg_w(base, NORMAL_INT_STATUS, INT_CMD_COMPLETE);
            break;
        }
        timeout -= 1;
    }
    if timeout == 0 {
        reset(base, SW_RESET_CMD);
        return 0xFFFF_FFFF;
    }

    reg_r(base, RESPONSE0)
}

pub(super) unsafe fn card_init(base: usize) -> Option<u32> {
    send_command(base, CMD_GO_IDLE_STATE, 0, CMD_RESP_NONE, false);

    let mut delay = 100_000u32;
    while delay > 0 {
        delay -= 1;
    }

    let resp8 = send_command(
        base,
        CMD_SEND_IF_COND,
        0x000001AA,
        CMD_RESP_48 | CMD_CRC_ENABLE | CMD_INDEX_ENABLE,
        false,
    );
    let sd_v2 = resp8 != 0xFFFF_FFFF && (resp8 & 0xFF) == 0xAA;

    let mut retries = 2000u32;
    let ocr = if sd_v2 { 0x40FF_8000 } else { 0x00FF_8000 };
    let mut op_cond_ok = false;
    while retries > 0 {
        let rca55 = send_command(
            base,
            CMD_APP_CMD,
            0,
            CMD_RESP_48 | CMD_CRC_ENABLE | CMD_INDEX_ENABLE,
            false,
        );
        if rca55 == 0xFFFF_FFFF {
            retries -= 1;
            continue;
        }

        let resp41 = send_command(
            base,
            ACMD_SD_SEND_OP_COND,
            ocr,
            CMD_RESP_48 | CMD_CRC_ENABLE,
            false,
        );
        if resp41 != 0xFFFF_FFFF && (resp41 & (1 << 31)) != 0 {
            op_cond_ok = true;
            break;
        }
        retries -= 1;
    }
    if !op_cond_ok {
        return None;
    }

    send_command(
        base,
        CMD_ALL_SEND_CID,
        0,
        CMD_RESP_136 | CMD_CRC_ENABLE,
        false,
    );

    let resp3 = send_command(
        base,
        CMD_SEND_RELATIVE_ADDR,
        0,
        CMD_RESP_48 | CMD_CRC_ENABLE | CMD_INDEX_ENABLE,
        false,
    );
    if resp3 == 0xFFFF_FFFF {
        return None;
    }
    let rca = resp3 >> 16;

    send_command(
        base,
        CMD_SEND_CSD,
        rca << 16,
        CMD_RESP_136 | CMD_CRC_ENABLE,
        false,
    );

    let resp7 = send_command(
        base,
        CMD_SELECT_CARD,
        rca << 16,
        CMD_RESP_48_BUSY | CMD_CRC_ENABLE | CMD_INDEX_ENABLE,
        false,
    );
    if resp7 == 0xFFFF_FFFF {
        return None;
    }

    let resp16 = send_command(
        base,
        CMD_SET_BLOCKLEN,
        SDHCI_SECTOR_SIZE as u32,
        CMD_RESP_48 | CMD_CRC_ENABLE | CMD_INDEX_ENABLE,
        false,
    );
    if resp16 == 0xFFFF_FFFF {
        return None;
    }

    reg_w(base, HOST_CONTROL, HC_DATA_WIDTH_4BIT | HC_HIGH_SPEED);

    Some(rca)
}

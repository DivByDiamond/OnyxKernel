use super::io::*;
use super::regs::*;

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

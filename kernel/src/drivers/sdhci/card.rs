use super::cmd::*;
use super::io::*;
use super::regs::*;

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

use crate::arch::mmio::Mmio;

pub(super) const SDMAS_SYS_ADDR: u32 = 0x00;
pub(super) const BLOCK_SIZE: u32 = 0x04;
pub(super) const BLOCK_COUNT: u32 = 0x06;
pub(super) const ARGUMENT: u32 = 0x08;
pub(super) const TRANSFER_MODE: u32 = 0x0C;
pub(super) const COMMAND: u32 = 0x0E;
pub(super) const RESPONSE0: u32 = 0x10;
pub(super) const _RESPONSE1: u32 = 0x14;
pub(super) const _RESPONSE2: u32 = 0x18;
pub(super) const _RESPONSE3: u32 = 0x1C;
pub(super) const BUFFER_DATA: u32 = 0x20;
pub(super) const PRESENT_STATE: u32 = 0x24;
pub(super) const HOST_CONTROL: u32 = 0x28;
pub(super) const POWER_CONTROL: u32 = 0x2C;
pub(super) const CLOCK_CONTROL: u32 = 0x30;
pub(super) const TIMEOUT_CONTROL: u32 = 0x34;
pub(super) const SOFTWARE_RESET: u32 = 0x38;
pub(super) const NORMAL_INT_STATUS: u32 = 0x3C;
pub(super) const ERROR_INT_STATUS: u32 = 0x40;
pub(super) const NORMAL_INT_STATUS_ENABLE: u32 = 0x44;
pub(super) const ERROR_INT_STATUS_ENABLE: u32 = 0x48;
pub(super) const _AUTO_CMD_ERROR: u32 = 0x50;

pub(super) const PS_CMD_INHIBIT: u32 = 1 << 0;
pub(super) const PS_DAT_INHIBIT: u32 = 1 << 1;
pub(super) const _PS_DAT_LINE_ACTIVE: u32 = 1 << 2;
pub(super) const _PS_WRITE_TRANSFER: u32 = 1 << 8;
pub(super) const _PS_READ_TRANSFER: u32 = 1 << 9;
pub(super) const PS_BUFFER_WRITE_ENABLE: u32 = 1 << 10;
pub(super) const PS_BUFFER_READ_ENABLE: u32 = 1 << 11;
pub(super) const PS_CARD_INSERTED: u32 = 1 << 16;
pub(super) const PS_CARD_STATE_STABLE: u32 = 1 << 17;

pub(super) const CMD_RESP_NONE: u16 = 0 << 0;
pub(super) const CMD_RESP_136: u16 = 1 << 0;
pub(super) const CMD_RESP_48: u16 = 2 << 0;
pub(super) const CMD_RESP_48_BUSY: u16 = 3 << 0;
pub(super) const CMD_CRC_ENABLE: u16 = 1 << 3;
pub(super) const CMD_INDEX_ENABLE: u16 = 1 << 4;
pub(super) const CMD_DATA_PRESENT: u16 = 1 << 5;

pub(super) const TM_READ: u16 = 1 << 4;
pub(super) const _TM_BLOCK_COUNT: u16 = 1 << 1;

pub(super) const SW_RESET_ALL: u32 = 1 << 0;
pub(super) const SW_RESET_CMD: u32 = 1 << 1;
pub(super) const SW_RESET_DAT: u32 = 1 << 2;

pub(super) const INT_CMD_COMPLETE: u32 = 1 << 0;
pub(super) const INT_TRANSFER_COMPLETE: u32 = 1 << 1;
pub(super) const INT_ERROR: u32 = 1 << 15;

pub(super) const _ERR_CMD_TIMEOUT: u32 = 1 << 0;
pub(super) const _ERR_CMD_CRC: u32 = 1 << 1;
pub(super) const _ERR_DAT_TIMEOUT: u32 = 1 << 4;
pub(super) const _ERR_DAT_CRC: u32 = 1 << 5;

pub(super) const CLK_INTERNAL_ENABLE: u32 = 1 << 0;
pub(super) const CLK_STABLE: u32 = 1 << 1;
pub(super) const CLK_SD_CLOCK_ENABLE: u32 = 1 << 2;
pub(super) const CLK_MAX_DIV: u32 = 0x3FF;

pub(super) const PWR_BUS_POWER: u32 = 1 << 0;
pub(super) const PWR_3_3V: u32 = 7 << 1;

pub(super) const HC_DATA_WIDTH_4BIT: u32 = 1 << 1;
pub(super) const HC_HIGH_SPEED: u32 = 1 << 2;

pub(super) const CMD_GO_IDLE_STATE: u16 = 0;
pub(super) const CMD_ALL_SEND_CID: u16 = 2;
pub(super) const CMD_SEND_RELATIVE_ADDR: u16 = 3;
pub(super) const _CMD_SET_DSR: u16 = 4;
pub(super) const CMD_SELECT_CARD: u16 = 7;
pub(super) const CMD_SEND_IF_COND: u16 = 8;
pub(super) const CMD_SEND_CSD: u16 = 9;
pub(super) const CMD_SET_BLOCKLEN: u16 = 16;
pub(super) const CMD_READ_SINGLE_BLOCK: u16 = 17;
pub(super) const CMD_READ_MULTIPLE_BLOCK: u16 = 18;
pub(super) const CMD_WRITE_SINGLE_BLOCK: u16 = 24;
pub(super) const CMD_WRITE_MULTIPLE_BLOCK: u16 = 25;
pub(super) const CMD_APP_CMD: u16 = 55;
pub(super) const ACMD_SD_SEND_OP_COND: u16 = 41;

pub(super) const SDHCI_SECTOR_SIZE: usize = 512;
pub(super) const SDHCI_TIMEOUT: u32 = 1_000_000;

pub const PLIC_PRIO_SDHCI: u32 = 0x07;

#[inline]
pub(super) unsafe fn reg_r(base: usize, off: u32) -> u32 {
    Mmio::<u32>::at(base + off as usize).read()
}

#[inline]
pub(super) unsafe fn reg_w(base: usize, off: u32, v: u32) {
    Mmio::<u32>::at(base + off as usize).write(v);
}

#[inline]
pub(super) unsafe fn reg_r16(base: usize, off: u32) -> u16 {
    Mmio::<u16>::at(base + off as usize).read()
}

#[inline]
pub(super) unsafe fn reg_w16(base: usize, off: u32, v: u16) {
    Mmio::<u16>::at(base + off as usize).write(v);
}

use crate::arch::mmio::Mmio;

pub const MAC_CFG: u32 = 0x00;
pub const MAC_ADDR0_LO: u32 = 0x04;
pub const MAC_ADDR0_HI: u32 = 0x08;
pub const MII_ADDR: u32 = 0x10;
pub const MII_DATA: u32 = 0x14;
pub const FLOW_CTRL: u32 = 0x18;
pub const VLAN_TAG: u32 = 0x1C;
pub const VERSION: u32 = 0x24;

pub const DMA_BUS_MODE: u32 = 0x1000;
pub const DMA_XMT_POLL_DEMAND: u32 = 0x1004;
pub const DMA_RCV_POLL_DEMAND: u32 = 0x1008;
pub const DMA_RX_BASE_ADDR: u32 = 0x100C;
pub const DMA_TX_BASE_ADDR: u32 = 0x1010;
pub const DMA_STATUS: u32 = 0x1014;
pub const DMA_OP_MODE: u32 = 0x1018;
pub const DMA_INT_ENABLE: u32 = 0x101C;
pub const DMA_MISSED_FRAME: u32 = 0x1020;
pub const DMA_TX_STATUS: u32 = 0x1040;
pub const DMA_RX_STATUS: u32 = 0x1044;

pub const MAC_CFG_RE: u32 = 1 << 2;
pub const MAC_CFG_TE: u32 = 1 << 3;
pub const MAC_CFG_PS: u32 = 1 << 4;
pub const MAC_CFG_DM: u32 = 1 << 8;
pub const MAC_CFG_LM: u32 = 1 << 9;
pub const MAC_CFG_FES: u32 = 1 << 14;
pub const MAC_CFG_SR: u32 = 1 << 0;

pub const MII_CR_42: u32 = 0b100 << 2;
pub const MII_W: u32 = 1 << 1;
pub const MII_B: u32 = 1 << 0;

pub const DMA_BUS_SWR: u32 = 1 << 0;
pub const DMA_BUS_FB: u32 = 1 << 1;
pub const DMA_BUS_MB: u32 = 1 << 17;
pub const DMA_BUS_AAL: u32 = 1 << 12;

pub const DMA_OP_SR: u32 = 1 << 1;
pub const DMA_OP_ST: u32 = 1 << 13;
pub const DMA_OP_TSF: u32 = 1 << 21;
pub const DMA_OP_RSF: u32 = 1 << 25;
pub const DMA_OP_OSF: u32 = 1 << 2;
pub const DMA_OP_FEF: u32 = 1 << 14;
pub const DMA_OP_FUF: u32 = 1 << 15;

pub const DMA_STA_NIS: u32 = 1 << 0;
pub const DMA_STA_AIS: u32 = 1 << 1;
pub const DMA_STA_ERI: u32 = 1 << 2;
pub const DMA_STA_FBI: u32 = 1 << 3;
pub const DMA_STA_TI: u32 = 1 << 7;
pub const DMA_STA_RPS: u32 = 1 << 5;
pub const DMA_STA_RWT: u32 = 1 << 8;
pub const DMA_STA_GMI: u32 = 1 << 9;
pub const DMA_STA_TSI: u32 = 1 << 16;

pub const TDES1_OWN: u32 = 1 << 31;
pub const TDES1_TER: u32 = 1 << 30;
pub const TDES1_TCH: u32 = 1 << 29;
pub const TDES1_LS: u32 = 1 << 28;
pub const TDES1_FS: u32 = 1 << 27;
pub const TDES1_IC: u32 = 1 << 26;
pub const TDES1_BS1_MASK: u32 = 0x1FFF;

pub const RDES1_OWN: u32 = 1 << 31;
pub const RDES1_TER: u32 = 1 << 30;
pub const RDES1_RCH: u32 = 1 << 29;
pub const RDES1_BS1_MASK: u32 = 0x1FFF;

pub const RDES0_FL_MASK: u32 = 0x3FFF << 16;

pub unsafe fn reg_r(base: usize, off: u32) -> u32 {
    Mmio::<u32>::at(base + off as usize).read()
}

pub unsafe fn reg_w(base: usize, off: u32, v: u32) {
    Mmio::<u32>::at(base + off as usize).write(v);
}

use crate::arch::mmio::Mmio;

pub const CAPLENGTH: u32 = 0x00;
pub const HCIVERSION: u32 = 0x02;
pub const HCSPARAMS1: u32 = 0x04;
pub const HCSPARAMS2: u32 = 0x08;
pub const HCCPARAMS1: u32 = 0x10;
pub const DBOFF: u32 = 0x14;
pub const RTSOFF: u32 = 0x18;

pub const OP_USBCMD: u32 = 0x00;
pub const OP_USBSTS: u32 = 0x04;
pub const OP_PAGESIZE: u32 = 0x08;
pub const OP_DNCTRL: u32 = 0x14;
pub const OP_CRCR: u32 = 0x18;
pub const OP_DCBAAP: u32 = 0x30;
pub const OP_CONFIG: u32 = 0x38;
pub const OP_PORTSC: u32 = 0x400;

pub const CMD_RUN: u32 = 1 << 0;
pub const CMD_HCRST: u32 = 1 << 1;
pub const CMD_INTES: u32 = 1 << 2;
pub const CMD_HSEE: u32 = 1 << 8;
pub const CMD_ETE: u32 = 1 << 11;

pub const STS_HCHALTED: u32 = 1 << 0;
pub const STS_CNR: u32 = 1 << 11;
pub const STS_HSE: u32 = 1 << 2;
pub const STS_EINT: u32 = 1 << 3;
pub const STS_PCD: u32 = 1 << 4;

pub const CRCR_ENABLE: u32 = 1 << 0;
pub const CRCR_CS: u32 = 1 << 1;
pub const CRCR_CA: u32 = 1 << 2;
pub const CRCR_RCS: u32 = 1 << 3;

pub const HCC_CSZ: u32 = 1 << 2;
pub const HCC_PAE: u32 = 1 << 8;
pub const HCC_SPS: u32 = 1 << 9;
pub const HCC_EXT_CAP_MASK: u32 = 0xFFFF << 16;

pub const PORT_CCS: u32 = 1 << 0;
pub const PORT_PED: u32 = 1 << 1;
pub const PORT_OCC: u32 = 1 << 3;
pub const PORT_PR: u32 = 1 << 4;
pub const PORT_PP: u32 = 1 << 9;
pub const PORT_CSC: u32 = 1 << 17;
pub const PORT_PEC: u32 = 1 << 18;
pub const PORT_WRC: u32 = 1 << 19;
pub const PORT_WPR: u32 = 1 << 31;

pub const PORT_SPEED_SHIFT: u32 = 20;
pub const PORT_SPEED_FS: u32 = 1 << PORT_SPEED_SHIFT;
pub const PORT_SPEED_LS: u32 = 2 << PORT_SPEED_SHIFT;
pub const PORT_SPEED_HS: u32 = 3 << PORT_SPEED_SHIFT;
pub const PORT_SPEED_SS: u32 = 4 << PORT_SPEED_SHIFT;

pub const DOORBELL_TARGET_SHIFT: u32 = 0;
pub const DOORBELL_TARGET_MASK: u32 = 0xFF;

pub const RTS_IMAN: u32 = 0x00;
pub const RTS_IMOD: u32 = 0x04;
pub const RTS_ERSTSZ: u32 = 0x08;
pub const RTS_ERSTBA: u32 = 0x10;
pub const RTS_ERDP: u32 = 0x18;

pub const IMAN_IP: u32 = 1 << 0;
pub const IMAN_IE: u32 = 1 << 1;

unsafe fn cap_r8(base: usize, off: u32) -> u8 {
    Mmio::<u8>::at(base + off as usize).read()
}

unsafe fn cap_r16(base: usize, off: u32) -> u16 {
    Mmio::<u16>::at(base + off as usize).read()
}

unsafe fn cap_r32(base: usize, off: u32) -> u32 {
    Mmio::<u32>::at(base + off as usize).read()
}

pub unsafe fn read_caplength(base: usize) -> u8 {
    cap_r8(base, CAPLENGTH)
}

pub unsafe fn read_hciversion(base: usize) -> u16 {
    cap_r16(base, HCIVERSION)
}

pub unsafe fn read_hcsparams1(base: usize) -> u32 {
    cap_r32(base, HCSPARAMS1)
}

pub unsafe fn read_hcsparams2(base: usize) -> u32 {
    cap_r32(base, HCSPARAMS2)
}

pub unsafe fn read_hccparams1(base: usize) -> u32 {
    cap_r32(base, HCCPARAMS1)
}

pub unsafe fn read_dboff(base: usize) -> u32 {
    cap_r32(base, DBOFF)
}

pub unsafe fn read_rtsoff(base: usize) -> u32 {
    cap_r32(base, RTSOFF)
}

pub unsafe fn op_r32(obase: usize, off: u32) -> u32 {
    Mmio::<u32>::at(obase + off as usize).read()
}

pub unsafe fn op_w32(obase: usize, off: u32, v: u32) {
    Mmio::<u32>::at(obase + off as usize).write(v);
}

pub unsafe fn op_w64(obase: usize, off: u32, v: u64) {
    Mmio::<u64>::at(obase + off as usize).write(v);
}

pub unsafe fn doorbell_w32(dboff: usize, slot: u8, target: u8) {
    Mmio::<u32>::at(dboff + (slot as usize) * 4).write(target as u32);
}

pub unsafe fn rt_r32(rtoff: usize, intr: u32, off: u32) -> u32 {
    Mmio::<u32>::at(rtoff + (intr as usize) * 0x20 + off as usize).read()
}

pub unsafe fn rt_w32(rtoff: usize, intr: u32, off: u32, v: u32) {
    Mmio::<u32>::at(rtoff + (intr as usize) * 0x20 + off as usize).write(v);
}

pub unsafe fn rt_w64(rtoff: usize, intr: u32, off: u32, v: u64) {
    Mmio::<u64>::at(rtoff + (intr as usize) * 0x20 + off as usize).write(v);
}

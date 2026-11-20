pub mod ring;
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

pub const CRCR_RCS: u32 = 1 << 0;
pub const CRCR_CS: u32 = 1 << 1;
pub const CRCR_CA: u32 = 1 << 2;
/// Read-only Command Ring Running indicator (§5.4.6 bit 3): never
/// written by software.
pub const CRCR_CRR: u32 = 1 << 3;

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

/// PORTSC bits [13:10]: port speed ID (xHCI §5.4.8) — the default
/// protocol mapping is 1 = full, 2 = low, 3 = high, 4 = SuperSpeed;
/// 0 is reserved. QEMU's qemu-xhci reports the same values
/// (PORTSC_SPEED_FULL..SUPER = 1..4 << 10). The slot-context speed
/// field takes the same ID (§6.2.2): the mapping is 1:1, no offset.
pub const PORT_SPEED_SHIFT: u32 = 10;
pub const PORT_SPEED_MASK: u32 = 0xF << PORT_SPEED_SHIFT;
pub const PORT_SPEED_ID_FS: u32 = 1;
pub const PORT_SPEED_ID_LS: u32 = 2;
pub const PORT_SPEED_ID_HS: u32 = 3;
pub const PORT_SPEED_ID_SS: u32 = 4;

pub const DOORBELL_TARGET_SHIFT: u32 = 0;
pub const DOORBELL_TARGET_MASK: u32 = 0xFF;

pub const RTS_IMAN: u32 = 0x00;
pub const RTS_IMOD: u32 = 0x04;
pub const RTS_ERSTSZ: u32 = 0x08;
pub const RTS_ERSTBA: u32 = 0x10;
pub const RTS_ERDP: u32 = 0x18;

pub const IMAN_IP: u32 = 1 << 0;
pub const IMAN_IE: u32 = 1 << 1;

/// # Safety
///
/// `base` must lie within the xHCI capability register file of a controller base validated at driver init; `off` must be a valid register offset.
unsafe fn cap_r8(base: usize, off: u32) -> u8 {
    // SAFETY: caller-validated MMIO base; offset within cap register file.
    unsafe { Mmio::<u8>::at(base + off as usize).read() }
}

/// # Safety
///
/// `base` must lie within the xHCI capability register file of a controller base validated at driver init; `off` must be a valid register offset.
unsafe fn cap_r32(base: usize, off: u32) -> u32 {
    // SAFETY: caller-validated MMIO base; offset within cap register file.
    unsafe { Mmio::<u32>::at(base + off as usize).read() }
}

/// # Safety
///
/// `base` must be the MMIO base of an xHCI controller validated at driver init and identity-mapped at boot; the offset is a fixed register-file offset per the constants in this file.
pub unsafe fn read_caplength(base: usize) -> u8 {
    // SAFETY: same contract as cap_r*: caller-validated base, fixed
    // register-file offset.
    unsafe { cap_r8(base, CAPLENGTH) }
}

/// # Safety
///
/// `base` must be the MMIO base of an xHCI controller validated at driver init and identity-mapped at boot; the offset is a fixed register-file offset per the constants in this file.
pub unsafe fn read_hciversion(base: usize) -> u16 {
    // SAFETY: same contract as cap_r*: caller-validated base, fixed
    // register-file offset. CAPLENGTH and HCIVERSION share the first dword;
    // QEMU implements only 4-byte accesses to the capability block (a 2-byte
    // read at 0x02 lands in its "unimplemented" path and returns 0), so read
    // the dword and take the upper half.
    unsafe { (cap_r32(base, CAPLENGTH) >> 16) as u16 }
}

/// # Safety
///
/// `base` must be the MMIO base of an xHCI controller validated at driver init and identity-mapped at boot; the offset is a fixed register-file offset per the constants in this file.
pub unsafe fn read_hcsparams1(base: usize) -> u32 {
    // SAFETY: same contract as cap_r*: caller-validated base, fixed
    // register-file offset.
    unsafe { cap_r32(base, HCSPARAMS1) }
}

/// # Safety
///
/// `base` must be the MMIO base of an xHCI controller validated at driver init and identity-mapped at boot; the offset is a fixed register-file offset per the constants in this file.
pub unsafe fn read_hcsparams2(base: usize) -> u32 {
    // SAFETY: same contract as cap_r*: caller-validated base, fixed
    // register-file offset.
    unsafe { cap_r32(base, HCSPARAMS2) }
}

/// # Safety
///
/// `base` must be the MMIO base of an xHCI controller validated at driver init and identity-mapped at boot; the offset is a fixed register-file offset per the constants in this file.
pub unsafe fn read_hccparams1(base: usize) -> u32 {
    // SAFETY: same contract as cap_r*: caller-validated base, fixed
    // register-file offset.
    unsafe { cap_r32(base, HCCPARAMS1) }
}

/// # Safety
///
/// `base` must be the MMIO base of an xHCI controller validated at driver init and identity-mapped at boot; the offset is a fixed register-file offset per the constants in this file.
pub unsafe fn read_dboff(base: usize) -> u32 {
    // SAFETY: same contract as cap_r*: caller-validated base, fixed
    // register-file offset.
    unsafe { cap_r32(base, DBOFF) }
}

/// # Safety
///
/// `base` must be the MMIO base of an xHCI controller validated at driver init and identity-mapped at boot; the offset is a fixed register-file offset per the constants in this file.
pub unsafe fn read_rtsoff(base: usize) -> u32 {
    // SAFETY: same contract as cap_r*: caller-validated base, fixed
    // register-file offset.
    unsafe { cap_r32(base, RTSOFF) }
}

/// # Safety
///
/// `base` must be the MMIO base of an xHCI controller validated at driver init and identity-mapped at boot; the offset is a fixed register-file offset per the constants in this file.
pub unsafe fn op_r32(obase: usize, off: u32) -> u32 {
    // SAFETY: caller-validated MMIO base; offset is a register-file constant.
    unsafe { Mmio::<u32>::at(obase + off as usize).read() }
}

/// # Safety
///
/// `base` must be the MMIO base of an xHCI controller validated at driver init and identity-mapped at boot; the offset is a fixed register-file offset per the constants in this file.
pub unsafe fn op_w32(obase: usize, off: u32, v: u32) {
    // SAFETY: base was validated at driver init and lies inside the
    // controller MMIO window; offset is within the register file.
    unsafe {
        Mmio::<u32>::at(obase + off as usize).write(v);
    }
}

/// # Safety
///
/// `base` must be the MMIO base of an xHCI controller validated at driver init and identity-mapped at boot; the offset is a fixed register-file offset per the constants in this file.
pub unsafe fn op_w64(obase: usize, off: u32, v: u64) {
    // QEMU's xHCI model (xhci_oper_write) dispatches on the full offset,
    // so a single 64-bit store lands in the LOW-word case only and the
    // high-word side effects never run (CRCR: the command ring is armed
    // on the high write; DCBAAP: the high half is never stored). QEMU
    // accepts 8-byte MMIO accesses as one dispatch, so 64-bit registers
    // must be written as two 32-bit stores, low then high.
    // SAFETY: base was validated at driver init and lies inside the
    // controller MMIO window; both offsets are within the register file.
    unsafe {
        Mmio::<u32>::at(obase + off as usize).write(v as u32);
        Mmio::<u32>::at(obase + off as usize + 4).write((v >> 32) as u32);
    }
}

/// # Safety
///
/// `base` must be the MMIO base of an xHCI controller validated at driver init and identity-mapped at boot; the offset is a fixed register-file offset per the constants in this file.
pub unsafe fn doorbell_w32(dboff: usize, slot: u8, target: u8) {
    // SAFETY: base was validated at driver init and lies inside the
    // controller MMIO window; offset is within the register file.
    unsafe {
        Mmio::<u32>::at(dboff + (slot as usize) * 4).write(target as u32);
    }
}

/// # Safety
///
/// `base` must be the MMIO base of an xHCI controller validated at driver init and identity-mapped at boot; the offset is a fixed register-file offset per the constants in this file.
pub unsafe fn rt_r32(rtoff: usize, intr: u32, off: u32) -> u32 {
    // SAFETY: caller-validated MMIO base; §5.5.2 puts MFINDEX in the
    // first 0x20 bytes, so interrupter set `intr` starts at
    // rtoff + 0x20 + intr*0x20 + off stays in the runtime register
    // set for intr 0.
    unsafe { Mmio::<u32>::at(rtoff + 0x20 + (intr as usize) * 0x20 + off as usize).read() }
}

/// # Safety
///
/// `base` must be the MMIO base of an xHCI controller validated at driver init and identity-mapped at boot; the offset is a fixed register-file offset per the constants in this file.
pub unsafe fn rt_w32(rtoff: usize, intr: u32, off: u32, v: u32) {
    // SAFETY: base was validated at driver init and lies inside the
    // controller MMIO window; offset is within the register file.
    unsafe {
        Mmio::<u32>::at(rtoff + 0x20 + (intr as usize) * 0x20 + off as usize).write(v);
    }
}

/// # Safety
///
/// `base` must be the MMIO base of an xHCI controller validated at driver init and identity-mapped at boot; the offset is a fixed register-file offset per the constants in this file.
pub unsafe fn rt_w64(rtoff: usize, intr: u32, off: u32, v: u64) {
    // SAFETY: base was validated at driver init and lies inside the
    // controller MMIO window; offset is within the register file.
    unsafe {
        rt_w32(rtoff, intr, off, v as u32);
        rt_w32(rtoff, intr, off + 4, (v >> 32) as u32);
    }
}

//! TrapFrame — register snapshot saved on kernel stack on trap entry.
//!
//! On 64-bit (rv64gc): 288 bytes, 36 u64 fields. Matches trap.S offsets.
//! On 32-bit (rv32gc): 144 bytes, 36 u32 fields. Matches trap_32.S offsets.

#![allow(non_snake_case)]

#[cfg(target_pointer_width = "64")]
pub const TRAP_FRAME_SIZE: usize = 288;

#[cfg(target_pointer_width = "32")]
pub const TRAP_FRAME_SIZE: usize = 144;

// Register-width type: u64 on rv64, u32 on rv32.
#[cfg(target_pointer_width = "64")]
pub type Reg = u64;
#[cfg(target_pointer_width = "32")]
pub type Reg = u32;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct TrapFrame {
    pub ra: Reg,
    pub sp: Reg,
    pub gp: Reg,
    pub tp: Reg,
    pub t0: Reg,
    pub t1: Reg,
    pub t2: Reg,
    pub s0: Reg,
    pub s1: Reg,
    pub a0: Reg,
    pub a1: Reg,
    pub a2: Reg,
    pub a3: Reg,
    pub a4: Reg,
    pub a5: Reg,
    pub a6: Reg,
    pub a7: Reg,
    pub s2: Reg,
    pub s3: Reg,
    pub s4: Reg,
    pub s5: Reg,
    pub s6: Reg,
    pub s7: Reg,
    pub s8: Reg,
    pub s9: Reg,
    pub s10: Reg,
    pub s11: Reg,
    pub t3: Reg,
    pub t4: Reg,
    pub t5: Reg,
    pub t6: Reg,
    pub sepc: Reg,
    pub sstatus: Reg,
    pub scause: Reg,
    pub stval: Reg,
    pub satp: Reg,
}

impl TrapFrame {
    pub const fn zero() -> Self {
        Self {
            ra: 0, sp: 0, gp: 0, tp: 0,
            t0: 0, t1: 0, t2: 0, s0: 0, s1: 0,
            a0: 0, a1: 0, a2: 0, a3: 0, a4: 0, a5: 0, a6: 0, a7: 0,
            s2: 0, s3: 0, s4: 0, s5: 0, s6: 0, s7: 0, s8: 0, s9: 0, s10: 0, s11: 0,
            t3: 0, t4: 0, t5: 0, t6: 0,
            sepc: 0, sstatus: 0, scause: 0, stval: 0, satp: 0,
        }
    }
}

// Compile-time layout assertions.
#[cfg(target_pointer_width = "32")]
const _: () = {
    use core::mem::{offset_of, size_of};
    assert!(size_of::<TrapFrame>() == TRAP_FRAME_SIZE);
    assert!(offset_of!(TrapFrame, ra) == 0);
    assert!(offset_of!(TrapFrame, sp) == 4); // 4 bytes (u32)
    assert!(offset_of!(TrapFrame, sepc) == 124);
    assert!(offset_of!(TrapFrame, sstatus) == 128);
    assert!(offset_of!(TrapFrame, satp) == 140);
};

#[cfg(target_pointer_width = "64")]
const _: () = {
    use core::mem::{offset_of, size_of};
    assert!(size_of::<TrapFrame>() == TRAP_FRAME_SIZE);
    assert!(offset_of!(TrapFrame, ra) == 0);
    assert!(offset_of!(TrapFrame, sp) == 8);
    assert!(offset_of!(TrapFrame, sepc) == 248);
    assert!(offset_of!(TrapFrame, sstatus) == 256);
    assert!(offset_of!(TrapFrame, satp) == 280);
};

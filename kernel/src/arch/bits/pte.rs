use super::PteVal;

#[cfg(target_pointer_width = "64")]
pub const SATP_MODE_SV39: u64 = 8 << 60;
pub const SATP_MODE_SV32: u32 = 1 << 31;

#[cfg(target_pointer_width = "64")]
pub const SATP_MODE_PAGING: PteVal = SATP_MODE_SV39;
#[cfg(target_pointer_width = "32")]
pub const SATP_MODE_PAGING: PteVal = SATP_MODE_SV32;

pub const PTE_V: PteVal = 1;
pub const PTE_R: PteVal = 2;
pub const PTE_W: PteVal = 4;
pub const PTE_X: PteVal = 8;
pub const PTE_U: PteVal = 16;
pub const PTE_G: PteVal = 32;
pub const PTE_A: PteVal = 64;
pub const PTE_D: PteVal = 128;
pub const PTE_LEAF: PteVal = PTE_R | PTE_X;

pub const PTE_PPN_SHIFT: PteVal = 10;

#[cfg(target_pointer_width = "64")]
pub const PTE_PPN_MASK: PteVal = ((1u64 << 44) - 1) << 10;
#[cfg(target_pointer_width = "32")]
pub const PTE_PPN_MASK: PteVal = ((1u32 << 20) - 1) << 10;

pub const PTE_FLAGS_MASK: PteVal = 0x3FF;

#[cfg(target_pointer_width = "64")]
pub const PTES_PER_TABLE: usize = 512;
#[cfg(target_pointer_width = "32")]
pub const PTES_PER_TABLE: usize = 1024;

#[cfg(target_pointer_width = "64")]
pub const PAGING_LEVELS: u32 = 3;
#[cfg(target_pointer_width = "32")]
pub const PAGING_LEVELS: u32 = 2;

pub const PAGE_SIZE: usize = 4096;
pub const PAGE_SHIFT: u32 = 12;

#[cfg(target_pointer_width = "64")]
#[inline]
pub const fn l2_idx(va: super::Va) -> usize {
    ((va >> 30) & 0x1FF) as usize
}

#[cfg(target_pointer_width = "64")]
#[inline]
pub const fn l1_idx(va: super::Va) -> usize {
    ((va >> 21) & 0x1FF) as usize
}
#[cfg(target_pointer_width = "32")]
#[inline]
pub const fn l1_idx(va: super::Va) -> usize {
    ((va >> 22) & 0x3FF) as usize
}

#[cfg(target_pointer_width = "64")]
#[inline]
pub const fn l0_idx(va: super::Va) -> usize {
    ((va >> 12) & 0x1FF) as usize
}
#[cfg(target_pointer_width = "32")]
#[inline]
pub const fn l0_idx(va: super::Va) -> usize {
    ((va >> 12) & 0x3FF) as usize
}

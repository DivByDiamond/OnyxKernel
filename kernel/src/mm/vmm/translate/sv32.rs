use crate::arch::bits;
use crate::arch::regs::*;
use core::ptr;

pub unsafe fn translate(root_pa: u64, vaddr: u64) -> u64 {
    let mut pa = root_pa;
    for level in (0..=1).rev() {
        let idx = match level {
            1 => bits::l1_idx(vaddr),
            0 => bits::l0_idx(vaddr),
            _ => return 0,
        };
        let pte = ptr::read_volatile((pa as usize + idx * 8) as *const u64);
        if pte & PTE_V == 0 {
            return 0;
        }
        if pte & PTE_LEAF != 0 {
            let leaf_ppn = (pte & PTE_PPN_MASK) >> PTE_PPN_SHIFT;
            let off = match level {
                1 => vaddr & ((1u64 << 22) - 1),
                0 => vaddr & ((1u64 << 12) - 1),
                _ => return 0,
            };
            return (leaf_ppn << 12) + off;
        }
        pa = (pte & PTE_PPN_MASK) >> PTE_PPN_SHIFT << 12;
    }
    0
}

pub unsafe fn translate_user(root_pa: u64, vaddr: u64) -> u64 {
    let mut pa = root_pa;
    for level in (0..=1).rev() {
        let idx = match level {
            1 => bits::l1_idx(vaddr),
            0 => bits::l0_idx(vaddr),
            _ => return 0,
        };
        let pte = ptr::read_volatile((pa as usize + idx * 8) as *const u64);
        if pte & PTE_V == 0 {
            return 0;
        }
        if pte & PTE_LEAF != 0 {
            if pte & PTE_U == 0 {
                return 0;
            }
            let leaf_ppn = (pte & PTE_PPN_MASK) >> PTE_PPN_SHIFT;
            let off = match level {
                1 => vaddr & ((1u64 << 22) - 1),
                0 => vaddr & ((1u64 << 12) - 1),
                _ => return 0,
            };
            return (leaf_ppn << 12) + off;
        }
        pa = (pte & PTE_PPN_MASK) >> PTE_PPN_SHIFT << 12;
    }
    0
}

pub unsafe fn translate_user_write(root_pa: u64, vaddr: u64) -> u64 {
    let mut pa = root_pa;
    for level in (0..=1).rev() {
        let idx = match level {
            1 => bits::l1_idx(vaddr),
            0 => bits::l0_idx(vaddr),
            _ => return 0,
        };
        let pte = ptr::read_volatile((pa as usize + idx * 8) as *const u64);
        if pte & PTE_V == 0 {
            return 0;
        }
        if pte & PTE_LEAF != 0 {
            if pte & (PTE_U | PTE_W) != (PTE_U | PTE_W) {
                return 0;
            }
            let leaf_ppn = (pte & PTE_PPN_MASK) >> PTE_PPN_SHIFT;
            let off = match level {
                1 => vaddr & ((1u64 << 22) - 1),
                0 => vaddr & ((1u64 << 12) - 1),
                _ => return 0,
            };
            return (leaf_ppn << 12) + off;
        }
        pa = (pte & PTE_PPN_MASK) >> PTE_PPN_SHIFT << 12;
    }
    0
}

pub unsafe fn pte_user_flags(root_pa: u64, vaddr: u64) -> u64 {
    let mut pa = root_pa;
    for level in (0..=1).rev() {
        let idx = match level {
            1 => bits::l1_idx(vaddr),
            0 => bits::l0_idx(vaddr),
            _ => return 0,
        };
        let pte = ptr::read_volatile((pa as usize + idx * 8) as *const u64);
        if pte & PTE_V == 0 {
            return 0;
        }
        if pte & PTE_LEAF != 0 {
            if pte & PTE_U == 0 {
                return 0;
            }
            return pte & PTE_FLAGS_MASK;
        }
        pa = (pte & PTE_PPN_MASK) >> PTE_PPN_SHIFT << 12;
    }
    0
}

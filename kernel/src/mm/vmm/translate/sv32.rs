use crate::arch::bits;
use crate::arch::regs::*;
use core::ptr;

/// Software Sv32 page-table walk returning the physical address that
/// `vaddr` resolves to (0 if unmapped).
///
/// # Safety
///
/// `root_pa` must be a page-aligned physical address of a live Sv32 root
/// table readable through the kernel's direct physical mapping, and every
/// non-leaf PTE on the walk must point to an allocated next-level table.
pub unsafe fn translate(root_pa: u64, vaddr: u64) -> u64 {
    if root_pa == 0 {
        return 0;
    }
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

/// Like [`translate`], but only succeeds for user-accessible leaves
/// (`PTE_U` set); returns 0 otherwise.
///
/// # Safety
///
/// Same contract as [`translate`].
pub unsafe fn translate_user(root_pa: u64, vaddr: u64) -> u64 {
    if root_pa == 0 {
        return 0;
    }
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

/// Like [`translate`], but only succeeds for user leaves that are also
/// writable (`PTE_U | PTE_W`); returns 0 otherwise.
///
/// # Safety
///
/// Same contract as [`translate`].
pub unsafe fn translate_user_write(root_pa: u64, vaddr: u64) -> u64 {
    if root_pa == 0 {
        return 0;
    }
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

/// Return the flag bits of the user leaf PTE covering `vaddr`
/// (0 if unmapped or not a user leaf).
///
/// # Safety
///
/// Same contract as [`translate`].
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

/// OR `add_flags` into an existing level-0 user PTE. Returns `false` if the
/// page is not mapped as a user leaf at level 0 (caller must then fall back
/// to a fresh mapping or report an error).
///
/// # Safety
///
/// Same contract as [`translate`]; additionally the caller must hold the
/// VMM lock so the read-modify-write of the PTE is atomic with respect to
/// other mappers.
pub unsafe fn update_user_pte(root_pa: u64, vaddr: u64, add_flags: u64) -> bool {
    let mut pa = root_pa;
    for level in (0..=1).rev() {
        let idx = match level {
            1 => bits::l1_idx(vaddr),
            0 => bits::l0_idx(vaddr),
            _ => return false,
        };
        let pte_ptr = (pa as usize + idx * 8) as *mut u64;
        let pte = ptr::read_volatile(pte_ptr);
        if pte & PTE_V == 0 {
            return false;
        }
        if pte & PTE_LEAF != 0 {
            if pte & PTE_U == 0 || level != 0 {
                return false;
            }
            ptr::write_volatile(pte_ptr, pte | add_flags);
            crate::arch::csr::sfence_vma(vaddr, 0);
            return true;
        }
        pa = (pte & PTE_PPN_MASK) >> PTE_PPN_SHIFT << 12;
    }
    false
}

//! Page-table walker for Sv32 (32-bit paging).
//!
//! Sv32 has 2 levels (vs Sv39's 3):
//!   L1: 1024 entries, each covering 4 MiB (22-bit VA offset)
//!   L0: 1024 entries, each covering 4 KiB (12-bit VA offset)
//!
//! PTE format (32-bit):
//!   [ppn(20) | rsw(2) | daguxwr(8) | v(1) | reserved(1)]
//!   - PPN is 20 bits (bits 10-29 of the PTE)
//!   - Bit 0 = V (valid)
//!   - Bits 1-8 = D/A/G/U/X/W/R
//!   - Bits 9-10 = RSW (reserved for software)
//!
//! SATP encoding (32-bit):
//!   bit 31 = MODE (1 = Sv32)
//!   bits 0-21 = PPN of root table (22 bits, since PA is 34-bit max
//!   in Sv32 — the PPN can address up to 2^22 * 4 KiB = 16 GiB, but
//!   physical RAM is typically <= 4 GiB on rv32)
//!
//! This module is compiled only when target_pointer_width = "32".
use crate::arch::regs::*;
use crate::mm::pmm;
use core::ptr;
use onyx_core::errno::{Errno, KResult};

/// Walk the Sv32 page table (2 levels: L1 then L0).
/// Mirrors the Sv39 walker in walk.rs but with only 2 levels.
pub(super) unsafe fn walk(
    root_pa: u64,
    vaddr: u64,
    leaf_level: u32,
    create: bool,
) -> KResult<*mut u64> {
    // Sv32 has 2 levels: L1 (4 MiB) and L0 (4 KiB).
    // leaf_level must be 0 or 1.
    if leaf_level > 1 {
        return Err(Errno::Inval);
    }
    let mut table_pa = root_pa;
    // Iterate from the level above leaf_level down to leaf_level.
    for level in (leaf_level + 1..=1).rev() {
        let idx = match level {
            1 => sv39_l1_idx(vaddr), // reuse — Sv32 L1 index is bits 22-31
            _ => return Err(Errno::Inval),
        };
        let pte_ptr = (table_pa as usize + idx * 8) as *mut u64;
        let pte = ptr::read_volatile(pte_ptr);
        if pte & PTE_V == 0 {
            if !create {
                return Err(Errno::NoEnt);
            }
            let new_pa = pmm::alloc_zero()?;
            ptr::write_volatile(pte_ptr, PTE_V | ((new_pa >> 12) << PTE_PPN_SHIFT));
            table_pa = new_pa;
        } else if pte & PTE_LEAF != 0 {
            if !create {
                return Err(Errno::Inval);
            }
            split_leaf(pte_ptr, pte, level)?;
            let new_pte = ptr::read_volatile(pte_ptr);
            table_pa = (new_pte & PTE_PPN_MASK) >> PTE_PPN_SHIFT << 12;
        } else {
            table_pa = (pte & PTE_PPN_MASK) >> PTE_PPN_SHIFT << 12;
        }
    }
    let idx = match leaf_level {
        0 => sv39_l0_idx(vaddr), // reuse — Sv32 L0 index is bits 12-21
        1 => sv39_l1_idx(vaddr),
        _ => return Err(Errno::Inval),
    };
    Ok((table_pa as usize + idx * 8) as *mut u64)
}

/// Split a leaf PTE into a non-leaf PTE pointing at a new table.
/// For Sv32, only level 1 (4 MiB) leaves can be split into 1024 × 4 KiB.
unsafe fn split_leaf(parent_pte_ptr: *mut u64, parent_pte: u64, parent_level: u32) -> KResult<()> {
    let new_pa = pmm::alloc_zero()?;
    let new_table = new_pa as *mut u64;
    let orig_ppn = (parent_pte & PTE_PPN_MASK) >> PTE_PPN_SHIFT;
    let flags = parent_pte & PTE_FLAGS_MASK;
    let shift = match parent_level {
        1 => 12u32, // 4 MiB → 4 KiB
        _ => return Err(Errno::Inval),
    };
    // Sv32: 1024 entries per table (vs Sv39's 512).
    for i in 0..1024u64 {
        let sub_pa = (orig_ppn << 12) + i * (1u64 << shift);
        ptr::write_volatile(
            new_table.add(i as usize),
            PTE_V | flags | ((sub_pa >> 12) << PTE_PPN_SHIFT),
        );
    }
    ptr::write_volatile(parent_pte_ptr, PTE_V | ((new_pa >> 12) << PTE_PPN_SHIFT));
    crate::arch::csr::sfence_vma_all();
    Ok(())
}

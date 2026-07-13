//! Page-table walker and leaf-splitting.
use crate::arch::regs::*;
use crate::mm::pmm;
use core::ptr;
use onyx_core::errno::{Errno, KResult};

/// Walk the page table from `root_pa` to the PTE for `vaddr` at `leaf_level`.
/// If `create` is true, allocates intermediate tables as needed and splits
/// leaf PTEs that would be too coarse for the requested level. Returns a
/// mutable pointer to the leaf PTE slot.
pub(super) unsafe fn walk(
    root_pa: u64,
    vaddr: u64,
    leaf_level: u32,
    create: bool,
) -> KResult<*mut u64> {
    let mut table_pa = root_pa;
    for level in (leaf_level + 1..=2).rev() {
        let idx = match level {
            2 => sv39_l2_idx(vaddr),
            1 => sv39_l1_idx(vaddr),
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
        0 => sv39_l0_idx(vaddr),
        1 => sv39_l1_idx(vaddr),
        2 => sv39_l2_idx(vaddr),
        _ => return Err(Errno::Inval),
    };
    Ok((table_pa as usize + idx * 8) as *mut u64)
}

/// Split a leaf PTE at `parent_pte_ptr` (originally `parent_pte`) into a
/// 512-entry intermediate table. `parent_level` is the level of the original
/// leaf (2 = 1 GiB, 1 = 2 MiB). The new table is written back to
/// `parent_pte_ptr` as a non-leaf PTE pointing at the freshly-allocated page.
///
/// Bug (mm SERIOUS #10): emit sfence.vma after the split so other harts don't
/// observe a stale 1 GiB / 2 MiB leaf PTE via a cached TLB entry — which would
/// bypass the new intermediate table and write directly to the old huge page.
unsafe fn split_leaf(parent_pte_ptr: *mut u64, parent_pte: u64, parent_level: u32) -> KResult<()> {
    let new_pa = pmm::alloc_zero()?;
    let new_table = new_pa as *mut u64;
    let orig_ppn = (parent_pte & PTE_PPN_MASK) >> PTE_PPN_SHIFT;
    let flags = parent_pte & PTE_FLAGS_MASK;
    let shift = match parent_level {
        2 => 21u32,
        1 => 12u32,
        _ => return Err(Errno::Inval),
    };
    for i in 0..512u64 {
        let sub_pa = (orig_ppn << 12) + i * (1u64 << shift);
        ptr::write_volatile(
            new_table.add(i as usize),
            PTE_V | flags | ((sub_pa >> 12) << PTE_PPN_SHIFT),
        );
    }
    ptr::write_volatile(parent_pte_ptr, PTE_V | ((new_pa >> 12) << PTE_PPN_SHIFT));
    // Invalidate any cached TLB entry for the VA range covered by the
    // original huge leaf — otherwise another hart could keep using the
    // old 1 GiB / 2 MiB mapping and miss the freshly-split table. We
    // don't know the exact VA here (walk does), so flush everything.
    crate::arch::csr::sfence_vma_all();
    Ok(())
}

use crate::arch::regs::*;
use crate::mm::pmm;
use core::ptr;
use onyx_core::errno::{Errno, KResult};

/// Walk an Sv39 page table down to the PTE slot for `vaddr` at
/// `leaf_level`, optionally allocating intermediate tables.
///
/// # Safety
///
/// `root_pa` must be a page-aligned physical address of a live Sv39 root
/// table accessible through the kernel's direct physical mapping. When
/// `create` is true the caller must hold the VMM lock so concurrent walkers
/// cannot observe half-built tables. The returned pointer aliases the page
/// table and is only valid while that table remains allocated and mapped.
pub unsafe fn walk(root_pa: u64, vaddr: u64, leaf_level: u32, create: bool) -> KResult<*mut u64> {
    // SAFETY: every volatile read/write targets `table_pa + idx*8` with
    // idx < 512, where `table_pa` starts at the (valid) root and each step
    // comes from a valid non-leaf PTE or a freshly zero-allocated table
    // from `pmm::alloc_zero`. Mutations require the VMM lock per contract.
    unsafe {
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
}

/// Split a mega-page leaf at `parent_level` into a full next-level table of
/// 4 KiB leaves, then repoint the parent PTE at the new table.
///
/// # Safety
///
/// Caller must hold the VMM lock. `parent_pte_ptr` must be a live PTE slot
/// (as returned by [`walk`]) whose value is `parent_pte`, a valid non-empty
/// leaf PTE at level 2 or 1; `orig_ppn << shift` arithmetic relies on the
/// leaf being aligned to its own block size, which the Sv39 spec guarantees.
unsafe fn split_leaf(parent_pte_ptr: *mut u64, parent_pte: u64, parent_level: u32) -> KResult<()> {
    // SAFETY: writes go to a freshly zero-allocated table (`i < 512`) and
    // to the validated parent PTE slot; volatile accesses keep the MMU-visible
    // ordering, and sfence_vma_all flushes stale translations afterwards.
    unsafe {
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
        crate::arch::csr::sfence_vma_all();
        Ok(())
    }
}

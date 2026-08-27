use crate::arch::regs::*;
use crate::mm::pmm;
use onyx_core::errno::{Errno, KResult};
use onyx_core::fmt::Arg;

#[cfg(target_pointer_width = "64")]
use super::walk::walk;
#[cfg(target_pointer_width = "32")]
use super::walk::walk;

/// Map `[paddr, paddr+size)` at `vaddr` in the address space rooted at
/// `root_pa`, using large pages where alignment permits.
///
/// # Safety
///
/// `root_pa` must be a live root table (direct-mapped); `vaddr`/`paddr`
/// must be page-aligned and the ranges must not overlap existing user
/// mappings (EEXIST is returned if they do). Interrupts disabled.
pub unsafe fn map(root_pa: u64, vaddr: u64, paddr: u64, size: usize, flags: u64) -> KResult<()> {
    // SAFETY: lock acquisition itself is safe; `map_impl` upholds the
    // contract above under the VMM lock.
    unsafe {
        super::lock::vmm_lock();
        let r = map_impl(root_pa, vaddr, paddr, size, flags);
        super::lock::vmm_unlock();
        r
    }
}

/// Locking variant of [`map`]. Caller MUST hold the VMM lock.
///
/// # Safety
///
/// Same as [`map`] plus: caller must hold the VMM lock. Each iteration
/// writes one PTE via [`map_one`], whose slot comes from a validated walk.
unsafe fn map_impl(root_pa: u64, vaddr: u64, paddr: u64, size: usize, flags: u64) -> KResult<()> {
    // SAFETY: all PTE writes happen inside `map_one` on walk-validated
    // slots; the loop arithmetic is bounded by `remaining`.
    unsafe {
        let mut va = vaddr;
        let mut pa = paddr;
        let mut remaining = size as u64;
        while remaining > 0 {
            let level = best_level(va, pa, remaining);
            let chunk: u64 = if level == 2 {
                1u64 << 30
            } else if level == 1 {
                #[cfg(target_pointer_width = "64")]
                {
                    1u64 << 21
                }
                #[cfg(target_pointer_width = "32")]
                {
                    1u64 << 22
                }
            } else {
                1u64 << 12
            };
            let chunk = chunk.min(remaining);
            map_one(root_pa, va, pa, flags | PTE_A | PTE_D, level)?;
            va += chunk;
            pa += chunk;
            remaining -= chunk;
        }
        Ok(())
    }
}

/// Map `size` bytes of freshly allocated zeroed pages at `vaddr`, charging
/// each page against the per-system user-memory budget. Rolls back every
/// allocation on failure.
///
/// # Safety
///
/// `root_pa` must be a live root table; `vaddr` must be page-aligned and
/// free of existing user mappings. `flags` must include `PTE_U`: rollback
/// and the budget refund in `unmap_impl` only release frames charged to the
/// user budget for user pages. Interrupts disabled.
pub unsafe fn map_anon(root_pa: u64, vaddr: u64, size: usize, flags: u64) -> KResult<()> {
    // SAFETY: lock acquisition itself is safe; the impl upholds the
    // contract above under the VMM lock.
    unsafe {
        super::lock::vmm_lock();
        let r = map_anon_impl(root_pa, vaddr, size, flags);
        super::lock::vmm_unlock();
        r
    }
}

/// Locking variant of [`map_anon`]. Caller MUST hold the VMM lock.
///
/// # Safety
///
/// Same as [`map_anon`] plus: caller must hold the VMM lock.
unsafe fn map_anon_impl(root_pa: u64, vaddr: u64, size: usize, flags: u64) -> KResult<()> {
    // SAFETY: VMM lock held; every PTE write goes through `map_one` on a
    // walk-validated slot and every failure path unmaps/frees the pages
    // allocated so far, so no partial state escapes.
    unsafe {
        let mut va = vaddr;
        let size_aligned = (size + 4095) & !4095;
        let mut remaining = size_aligned as u64;
        while remaining > 0 {
            // Reserve against USER_MEM_MAX_BYTES before touching the PMM so a
            // runaway mmap fails cleanly with ENOMEM instead of draining RAM.
            if let Err(e) = crate::proc::limits::user_page_take() {
                rollback_mapped(root_pa, vaddr, va);
                return Err(e);
            }
            let page_pa = match pmm::alloc_zero() {
                Ok(pa) => pa,
                Err(e) => {
                    // The take above succeeded but no page was mapped — undo it
                    // along with every previously mapped page of this call.
                    crate::proc::limits::user_pages_release(1);
                    rollback_mapped(root_pa, vaddr, va);
                    return Err(e);
                }
            };
            if let Err(e) = map_one(root_pa, va, page_pa, flags | PTE_A | PTE_D, 0) {
                pmm::free(page_pa);
                crate::proc::limits::user_pages_release(1);
                rollback_mapped(root_pa, vaddr, va);
                return Err(e);
            }
            va += 1u64 << 12;
            remaining -= 1u64 << 12;
        }
        Ok(())
    }
}

/// Unwind the `[start, end)` prefix of a failed `map_anon` call. The range
/// was mapped page-by-page at level 0, so unwinding through the unmap path
/// releases both the frames and the per-page user-memory budget exactly as
/// a successful `unmap` would — for any number of pages, with no fixed
/// tracking limit.
///
/// # Safety
///
/// Caller must hold the VMM lock; `[start, end)` must only contain pages
/// mapped by the current `map_anon` call.
unsafe fn rollback_mapped(root_pa: u64, start: u64, end: u64) {
    if end > start {
        // SAFETY: VMM lock held; the whole range was mapped by this call
        // at level 0 with user flags, matching the `unmap_impl` contract.
        // The result is always `Ok` (per-page walk failures are skipped
        // inside), so discarding it is safe.
        unsafe {
            let _ = super::unmap::unmap_impl(root_pa, start, (end - start) as usize);
        };
    }
}

/// Write a single PTE for (`vaddr`, `paddr`) at `level`, creating
/// intermediate tables as needed.
///
/// # Safety
///
/// Caller must hold the VMM lock. `root_pa` must be a live root table;
/// `paddr` must be aligned to the block size implied by `level` (checked
/// below); `flags` must not contain reserved/illegal PTE bits.
unsafe fn map_one(root_pa: u64, vaddr: u64, paddr: u64, flags: u64, level: u32) -> KResult<()> {
    // SAFETY: VMM lock held; `walk(.., true)` returns a pointer to a live
    // PTE slot (see its contract), and both volatile accesses below target
    // exactly that slot. sfence_vma closes the stale-TLB window.
    unsafe {
        #[cfg(target_pointer_width = "64")]
        if level == 1 && paddr & ((1u64 << 21) - 1) != 0 {
            return Err(Errno::Inval);
        }
        #[cfg(target_pointer_width = "32")]
        if level == 1 && paddr & ((1u64 << 22) - 1) != 0 {
            return Err(Errno::Inval);
        }
        if level == 2 && paddr & ((1u64 << 30) - 1) != 0 {
            return Err(Errno::Inval);
        }
        let pte_ptr = walk(root_pa, vaddr, level, true)?;
        let old_pte = core::ptr::read_volatile(pte_ptr);
        if old_pte & PTE_V != 0 && old_pte & PTE_U != 0 {
            // A real user mapping already exists at this address — refuse to
            // clobber it (shared segment pages are handled by the caller via
            // update_user_pte). Non-user leaves (identity placeholder pages
            // produced by leaf splitting) are freely replaced below.
            crate::kinf!(
                "vmm",
                "map EEXIST vaddr=%p level=%d old_pte=%p",
                Arg::from(vaddr),
                Arg::from(level as u64),
                Arg::from(old_pte)
            );
            return Err(Errno::Exist);
        }
        let pte = PTE_V | flags | ((paddr >> 12) << PTE_PPN_SHIFT);
        core::ptr::write_volatile(pte_ptr, pte);
        crate::arch::csr::sfence_vma(vaddr, 0);
        Ok(())
    }
}

/// Public, self-locking wrapper around [`map_one`] for one-shot mappings.
///
/// # Safety
///
/// Same contract as [`map_one`]; interrupts disabled (spinlock invariant).
pub unsafe fn map_one_pub(
    root_pa: u64,
    vaddr: u64,
    paddr: u64,
    flags: u64,
    level: u32,
) -> KResult<()> {
    // SAFETY: lock acquisition itself is safe; `map_one` upholds its
    // contract under the VMM lock.
    unsafe {
        super::lock::vmm_lock();
        let r = map_one(root_pa, vaddr, paddr, flags, level);
        super::lock::vmm_unlock();
        r
    }
}

#[cfg(target_pointer_width = "64")]
fn best_level(va: u64, pa: u64, remaining: u64) -> u32 {
    if remaining >= (1u64 << 30) && (va & ((1u64 << 30) - 1)) == 0 && (pa & ((1u64 << 30) - 1)) == 0
    {
        return 2;
    }
    if remaining >= (1u64 << 21) && (va & ((1u64 << 21) - 1)) == 0 && (pa & ((1u64 << 21) - 1)) == 0
    {
        return 1;
    }
    0
}

#[cfg(target_pointer_width = "32")]
fn best_level(va: u64, pa: u64, remaining: u64) -> u32 {
    if remaining >= (1u64 << 22) && (va & ((1u64 << 22) - 1)) == 0 && (pa & ((1u64 << 22) - 1)) == 0
    {
        return 1;
    }
    0
}

mod user_copy;

pub use user_copy::{check_user_range, copy_from_user, copy_to_user};

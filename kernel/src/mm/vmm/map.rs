//! Map operations — `map`, `map_anon`, `map_one`, `map_one_pub`, `best_level`.
use crate::arch::regs::*;
use crate::mm::pmm;
use core::ptr;
use onyx_core::errno::KResult;

use super::walk::walk;

pub unsafe fn map(root_pa: u64, vaddr: u64, paddr: u64, size: usize, flags: u64) -> KResult<()> {
    super::vmm_lock();
    let r = map_impl(root_pa, vaddr, paddr, size, flags);
    super::vmm_unlock();
    r
}

unsafe fn map_impl(root_pa: u64, vaddr: u64, paddr: u64, size: usize, flags: u64) -> KResult<()> {
    let mut va = vaddr;
    let mut pa = paddr;
    let mut remaining = size as u64;
    while remaining > 0 {
        let level = best_level(va, pa, remaining);
        let chunk = if level == 2 {
            1u64 << 30
        } else if level == 1 {
            1u64 << 21
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

pub unsafe fn map_anon(root_pa: u64, vaddr: u64, size: usize, flags: u64) -> KResult<()> {
    super::vmm_lock();
    let r = map_anon_impl(root_pa, vaddr, size, flags);
    super::vmm_unlock();
    r
}

unsafe fn map_anon_impl(root_pa: u64, vaddr: u64, size: usize, flags: u64) -> KResult<()> {
    let mut va = vaddr;
    // Bug #5 fix: page-align size up so the subtraction `remaining -= 4096`
    // can't underflow when the caller passes a non-page-multiple size.
    // Without this, a size like 5000 would loop forever in release builds
    // (underflow wraps to a huge u64) or panic in debug builds.
    let size_aligned = (size + 4095) & !4095;
    let mut remaining = size_aligned as u64;
    // Bug (mm SERIOUS #8): track every page we allocate so that if a later
    // map_one() fails (e.g. PMM runs out of pages mid-mapping) we can free
    // the already-allocated pages and return a clean ENOMEM — instead of
    // leaking them. Previously every page allocated before the failure
    // was permanently lost.
    let mut allocated: [u64; 1024] = [0; 1024];
    let mut n_allocated: usize = 0;
    while remaining > 0 {
        let page_pa = match pmm::alloc_zero() {
            Ok(pa) => pa,
            Err(e) => {
                // Roll back: free every page we allocated above.
                for i in 0..n_allocated {
                    pmm::free(allocated[i]);
                }
                return Err(e);
            }
        };
        if let Err(e) = map_one(root_pa, va, page_pa, flags | PTE_A | PTE_D, 0) {
            // map_one failed — free this page AND every prior page.
            pmm::free(page_pa);
            for i in 0..n_allocated {
                pmm::free(allocated[i]);
            }
            return Err(e);
        }
        if n_allocated < allocated.len() {
            allocated[n_allocated] = page_pa;
            n_allocated += 1;
        }
        va += 1u64 << 12;
        remaining -= 1u64 << 12;
    }
    Ok(())
}

unsafe fn map_one(root_pa: u64, vaddr: u64, paddr: u64, flags: u64, level: u32) -> KResult<()> {
    // Bug (mm SERIOUS #16): for huge leaves (level 1 or 2), the physical
    // address MUST be naturally aligned to the leaf size (2 MiB / 1 GiB).
    // The Sv39 spec requires this for huge leaves — an unaligned PA
    // silently produces a corrupted mapping. Reject early instead.
    if level == 1 && paddr & ((1u64 << 21) - 1) != 0 {
        return Err(Errno::Inval);
    }
    if level == 2 && paddr & ((1u64 << 30) - 1) != 0 {
        return Err(Errno::Inval);
    }
    let pte_ptr = walk(root_pa, vaddr, level, true)?;
    // Bug (mm SERIOUS #15): check whether the PTE was already valid before
    // overwriting it. The previous code unconditionally wrote, which would
    // leak the page that the old PTE pointed at (caller's page_pa would
    // never be freed, and the old mapping's page would never be reclaimed).
    // We now refuse to overwrite a valid PTE — callers must unmap first.
    let old_pte = core::ptr::read_volatile(pte_ptr);
    if old_pte & PTE_V != 0 {
        return Err(Errno::Exist);
    }
    let pte = PTE_V | flags | ((paddr >> 12) << PTE_PPN_SHIFT);
    core::ptr::write_volatile(pte_ptr, pte);
    // Bug (mm SERIOUS #11): invalidate any stale TLB entry for this VA so
    // other harts see the new mapping immediately rather than continuing
    // to fault on the old (not-present) translation.
    crate::arch::csr::sfence_vma(vaddr, 0);
    Ok(())
}

pub unsafe fn map_one_pub(
    root_pa: u64,
    vaddr: u64,
    paddr: u64,
    flags: u64,
    level: u32,
) -> KResult<()> {
    super::vmm_lock();
    let r = map_one(root_pa, vaddr, paddr, flags, level);
    super::vmm_unlock();
    r
}

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

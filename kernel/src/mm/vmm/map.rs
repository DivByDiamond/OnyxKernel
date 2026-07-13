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
    while remaining > 0 {
        let page_pa = pmm::alloc_zero()?;
        map_one(root_pa, va, page_pa, flags | PTE_A | PTE_D, 0)?;
        va += 1u64 << 12;
        remaining -= 1u64 << 12;
    }
    Ok(())
}

unsafe fn map_one(root_pa: u64, vaddr: u64, paddr: u64, flags: u64, level: u32) -> KResult<()> {
    let pte_ptr = walk(root_pa, vaddr, level, true)?;
    let pte = PTE_V | flags | ((paddr >> 12) << PTE_PPN_SHIFT);
    ptr::write_volatile(pte_ptr, pte);
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

use crate::arch::regs::*;
use crate::mm::pmm;
use onyx_core::errno::{Errno, KResult};
use onyx_core::fmt::Arg;

#[cfg(target_pointer_width = "64")]
use super::walk::walk;
#[cfg(target_pointer_width = "32")]
use super::walk::walk;

pub unsafe fn map(root_pa: u64, vaddr: u64, paddr: u64, size: usize, flags: u64) -> KResult<()> { unsafe {
    super::lock::vmm_lock();
    let r = map_impl(root_pa, vaddr, paddr, size, flags);
    super::lock::vmm_unlock();
    r
}}

unsafe fn map_impl(root_pa: u64, vaddr: u64, paddr: u64, size: usize, flags: u64) -> KResult<()> { unsafe {
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
}}

pub unsafe fn map_anon(root_pa: u64, vaddr: u64, size: usize, flags: u64) -> KResult<()> { unsafe {
    super::lock::vmm_lock();
    let r = map_anon_impl(root_pa, vaddr, size, flags);
    super::lock::vmm_unlock();
    r
}}

unsafe fn map_anon_impl(root_pa: u64, vaddr: u64, size: usize, flags: u64) -> KResult<()> { unsafe {
    let mut va = vaddr;
    let size_aligned = (size + 4095) & !4095;
    let mut remaining = size_aligned as u64;
    let mut allocated: [u64; 1024] = [0; 1024];
    let mut n_allocated: usize = 0;
    // Pages accounted against the system-wide user-memory budget so far.
    let mut n_counted: usize = 0;
    while remaining > 0 {
        // Reserve against USER_MEM_MAX_BYTES before touching the PMM so a
        // runaway mmap fails cleanly with ENOMEM instead of draining RAM.
        if let Err(e) = crate::proc::limits::user_page_take() {
            for &pa in &allocated[..n_allocated] {
                pmm::free(pa);
            }
            crate::proc::limits::user_pages_release(n_counted);
            return Err(e);
        }
        let page_pa = match pmm::alloc_zero() {
            Ok(pa) => pa,
            Err(e) => {
                // The take above succeeded but no page was mapped — undo it
                // along with every previously counted page of this call.
                crate::proc::limits::user_pages_release(n_counted + 1);
                for &pa in &allocated[..n_allocated] {
                    pmm::free(pa);
                }
                return Err(e);
            }
        };
        if let Err(e) = map_one(root_pa, va, page_pa, flags | PTE_A | PTE_D, 0) {
            pmm::free(page_pa);
            crate::proc::limits::user_pages_release(n_counted + 1);
            for &pa in &allocated[..n_allocated] {
                pmm::free(pa);
            }
            return Err(e);
        }
        if n_allocated < allocated.len() {
            allocated[n_allocated] = page_pa;
            n_allocated += 1;
        }
        n_counted += 1;
        va += 1u64 << 12;
        remaining -= 1u64 << 12;
    }
    Ok(())
}}

unsafe fn map_one(root_pa: u64, vaddr: u64, paddr: u64, flags: u64, level: u32) -> KResult<()> { unsafe {
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
}}

pub unsafe fn map_one_pub(
    root_pa: u64,
    vaddr: u64,
    paddr: u64,
    flags: u64,
    level: u32,
) -> KResult<()> { unsafe {
    super::lock::vmm_lock();
    let r = map_one(root_pa, vaddr, paddr, flags, level);
    super::lock::vmm_unlock();
    r
}}

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

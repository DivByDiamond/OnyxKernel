use crate::arch::csr;
use crate::arch::regs::*;
use crate::mm::pmm;
use core::ptr;
use onyx_core::errno::KResult;

use super::walk::walk;

pub unsafe fn unmap(root_pa: u64, vaddr: u64, size: usize) -> KResult<()> {
    super::vmm_lock();
    let r = unmap_impl(root_pa, vaddr, size);
    super::vmm_unlock();
    r
}

unsafe fn unmap_impl(root_pa: u64, vaddr: u64, size: usize) -> KResult<()> {
    let mut va = vaddr;
    // Bug #5 fix: page-align size up so `remaining -= 4096` can't underflow
    // when size is not a multiple of PAGE_SIZE.
    let size_aligned = (size + 4095) & !4095;
    let mut remaining = size_aligned;
    while remaining > 0 {
        // Bug (mm SERIOUS #7): try to handle huge-page leaves. The previous
        // code always called walk(level=0), which on a huge leaf (1 GiB or
        // 2 MiB) would return an error because walk() refuses to split a
        // leaf in non-create mode. As a result, unmap() on any VA covered
        // by a huge leaf would bail out with NoEnt and leak the mapping.
        // We now walk at level 0 first; if that fails with NoEnt, retry
        // at levels 1 and 2 to detect a huge leaf and clear it directly.
        let pte_ptr = match walk(root_pa, va, 0, false) {
            Ok(p) => p,
            Err(_) => {
                // Try level 1 (2 MiB leaf)
                match walk(root_pa, va, 1, false) {
                    Ok(p) => p,
                    Err(_) => {
                        // Try level 2 (1 GiB leaf)
                        match walk(root_pa, va, 2, false) {
                            Ok(p) => p,
                            // No mapping at any level — nothing to unmap.
                            Err(_) => {
                                va += 4096;
                                remaining -= 4096;
                                continue;
                            }
                        }
                    }
                }
            }
        };
        let pte = ptr::read_volatile(pte_ptr);
        if pte & PTE_V != 0 && pte & PTE_U != 0 {
            let paddr = (pte & PTE_PPN_MASK) >> PTE_PPN_SHIFT << 12;
            if pmm::is_managed(paddr) {
                pmm::free(paddr);
            }
        }
        ptr::write_volatile(pte_ptr, 0);
        csr::sfence_vma(va, 0);
        va += 4096;
        remaining -= 4096;
    }
    Ok(())
}

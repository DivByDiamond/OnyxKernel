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
        let pte_ptr = walk(root_pa, va, 0, false)?;
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

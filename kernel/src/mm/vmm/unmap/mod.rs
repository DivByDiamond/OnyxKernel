use crate::arch::csr;
use crate::arch::regs::*;
use crate::mm::pmm;
use core::ptr;
use onyx_core::errno::KResult;

#[cfg(target_pointer_width = "64")]
use super::walk::walk;
#[cfg(target_pointer_width = "32")]
use super::walk::walk;

/// Unmap and free every 4 KiB page in `[vaddr, vaddr+size)`, releasing
/// user pages back to the PMM (and the user-memory budget).
///
/// # Safety
///
/// `root_pa` must be a live root table; `[vaddr, vaddr+size)` must not be
/// concurrently mapped/unmapped by another hart. Interrupts disabled.
pub unsafe fn unmap(root_pa: u64, vaddr: u64, size: usize) -> KResult<()> {
    // SAFETY: lock acquisition itself is safe; `unmap_impl` upholds the
    // contract above under the VMM lock.
    unsafe {
        super::lock::vmm_lock();
        let r = unmap_impl(root_pa, vaddr, size);
        super::lock::vmm_unlock();
        r
    }
}

/// Locking variant of [`unmap`]. Caller MUST hold the VMM lock.
///
/// # Safety
///
/// Same as [`unmap`] plus: caller must hold the VMM lock so PTE reads,
/// page frees and the clearing write below are atomic with respect to
/// other mappers.
unsafe fn unmap_impl(root_pa: u64, vaddr: u64, size: usize) -> KResult<()> {
    // SAFETY: VMM lock held. Each `walk(.., false)` returns a live PTE
    // slot per its contract; the volatile read/write pair plus sfence_vma
    // atomically retire one mapping, and `pmm::free` is only called for
    // PMM-managed user pages.
    unsafe {
        let mut va = vaddr;
        let size_aligned = (size + 4095) & !4095;
        let mut remaining = size_aligned;
        while remaining > 0 {
            let pte_ptr = match walk(root_pa, va, 0, false) {
                Ok(p) => p,
                Err(_) => match walk(root_pa, va, 1, false) {
                    Ok(p) => p,
                    Err(_) => {
                        #[cfg(target_pointer_width = "64")]
                        match walk(root_pa, va, 2, false) {
                            Ok(p) => p,
                            Err(_) => {
                                va += 4096;
                                remaining -= 4096;
                                continue;
                            }
                        }
                        #[cfg(target_pointer_width = "32")]
                        {
                            va += 4096;
                            remaining -= 4096;
                            continue;
                        }
                    }
                },
            };
            let pte = ptr::read_volatile(pte_ptr);
            if pte & PTE_V != 0 && pte & PTE_U != 0 {
                let paddr = (pte & PTE_PPN_MASK) >> PTE_PPN_SHIFT << 12;
                if pmm::is_managed(paddr) {
                    pmm::free(paddr);
                    // User pages freed here were accounted at map time
                    // (mmap / brk growth / exec load).
                    crate::proc::limits::user_pages_release(1);
                }
            }
            ptr::write_volatile(pte_ptr, 0);
            csr::sfence_vma(va, 0);
            va += 4096;
            remaining -= 4096;
        }
        Ok(())
    }
}

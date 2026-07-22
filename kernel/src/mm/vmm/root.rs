//! VMM — Sv39 paging with leaf-splitting.
//!
//! This is the directory root. It owns the kernel root page-table pointer
//! (`G_KERNEL_ROOT_PA`), the `new_root`/`install_root`/`init`/`kernel_root`
//! lifecycle helpers, `destroy_root` (with `free_subtree`), and the
//! `translate`/`translate_user` walkers. Map operations live in `map.rs`;
//! the page-table walker and leaf-splitting live in `walk.rs`.
use crate::arch::csr;
use crate::arch::regs::*;
use crate::mm::pmm;
use core::ptr;
use onyx_core::errno::KResult;

pub(super) static mut G_KERNEL_ROOT_PA: u64 = 0;

pub unsafe fn new_root() -> KResult<u64> {
    pmm::alloc_zero()
}

pub unsafe fn install_root(root_pa: u64) {
    #[cfg(target_pointer_width = "64")]
    {
        csr::write_satp(crate::arch::regs::SATP_MODE_SV39 | (root_pa >> 12));
    }
    #[cfg(target_pointer_width = "32")]
    {
        let satp = crate::arch::bits::SATP_MODE_SV32 | ((root_pa >> 12) & 0x3FFFFF) as u32;
        csr::write_satp(satp as u64);
    }
    csr::sfence_vma_all();
}

pub unsafe fn init() -> KResult<u64> {
    let root_pa = new_root()?;
    crate::arch::smp::G_KERNEL_ROOT_PA = root_pa;
    let root = root_pa as *mut u64;
    let leaf_flags = PTE_V | PTE_R | PTE_W | PTE_X | PTE_A | PTE_D;
    #[cfg(target_pointer_width = "64")]
    {
        for i in 0..3u64 {
            let pa = i << 30;
            ptr::write_volatile(
                root.add(i as usize),
                PTE_V | leaf_flags | (pa >> 12 << PTE_PPN_SHIFT),
            );
        }
    }
    #[cfg(target_pointer_width = "32")]
    {
        ptr::write_volatile(
            root.add(0),
            PTE_V | leaf_flags | (0u64 >> 12 << PTE_PPN_SHIFT),
        );
    }
    let p = &raw mut G_KERNEL_ROOT_PA;
    *p = root_pa;
    install_root(root_pa);
    Ok(root_pa)
}

pub fn kernel_root() -> u64 {
    unsafe { *(&raw const G_KERNEL_ROOT_PA) }
}

pub unsafe fn destroy_root(root_pa: u64) {
    super::lock::vmm_lock();
    let root = root_pa as *mut u64;
    #[cfg(target_pointer_width = "64")]
    free_subtree(root, 2);
    #[cfg(target_pointer_width = "32")]
    free_subtree(root, 1);
    super::lock::vmm_unlock();
    pmm::free(root_pa);
    csr::sfence_vma_all();
}

unsafe fn free_subtree(table: *mut u64, level: u32) {
    #[cfg(target_pointer_width = "64")]
    let entries = SV39_PTES_PER_TABLE;
    #[cfg(target_pointer_width = "32")]
    let entries = crate::arch::bits::PTES_PER_TABLE;
    for i in 0..entries {
        let pte = ptr::read_volatile(table.add(i));
        if pte & PTE_V == 0 {
            continue;
        }
        let is_leaf = pte & PTE_LEAF != 0;
        let child_pa = (pte & PTE_PPN_MASK) >> PTE_PPN_SHIFT << 12;
        if is_leaf {
            if pmm::is_managed(child_pa) {
                pmm::free(child_pa);
            }
        } else if level > 0 {
            free_subtree(child_pa as *mut u64, level - 1);
            pmm::free(child_pa);
        }
    }
}

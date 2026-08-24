use super::heap::{self, HEAP_SIZE, MIN_BLOCK};
use super::pmm::{self, KERNEL_HEAP_RESERVE, PAGE_SIZE, SLAB_SIZES};

#[test]
fn test_page_size() {
    assert_eq!(PAGE_SIZE, 4096);
}

#[test]
fn test_slab_sizes() {
    assert_eq!(SLAB_SIZES, [64, 256, 1024]);
}

#[test]
fn test_kernel_heap_reserve() {
    assert_eq!(KERNEL_HEAP_RESERVE, 4 * 1024 * 1024);
}

#[test]
fn test_heap_constants() {
    assert_eq!(HEAP_SIZE, 4 * 1024 * 1024);
    assert_eq!(MIN_BLOCK, 16);
}

#[test]
fn test_pmm_initial_state() {
    assert_eq!(pmm::free_pages(), 0);
    assert_eq!(pmm::total_pages(), 0);
    assert!(!pmm::is_managed(0));
    assert!(!pmm::is_managed(0x1000));
    assert!(!pmm::is_managed(0x8000_0000));
    assert!(!pmm::is_managed(!0u64 & !0xFFF));
}

#[test]
fn test_pmm_is_managed_non_aligned() {
    assert!(!pmm::is_managed(1));
    assert!(!pmm::is_managed(0xFFF));
    assert!(!pmm::is_managed(0x1001));
}

#[test]
fn test_heap_used_initial() {
    assert_eq!(heap::used(), 0);
}

#[test]
fn test_vmm_translate_empty() {
    // SAFETY: `translate` only reads PTE slots of the stack-resident fake
    // table; all-zero entries terminate the walk immediately.
    unsafe {
        let pt = [0u64; 512];
        let result = crate::mm::vmm::translate(pt.as_ptr() as u64, 0x1000);
        assert_eq!(result, 0);
    }
}

#[test]
fn test_vmm_translate_leaf() {
    // PTE entries only encode 4 KiB-aligned next-table addresses, so the
    // fake tables must themselves be page-aligned.
    #[repr(align(4096))]
    struct Page([u64; 512]);
    // SAFETY: all three fake tables are stack-resident and fully
    // initialised before the walk; PTEs reference each other by valid
    // page-aligned addresses, so every volatile read stays in bounds.
    unsafe {
        let mut l2 = Page([0u64; 512]);
        let mut l1 = Page([0u64; 512]);
        let mut l0 = Page([0u64; 512]);
        let vaddr = 0x1000u64;
        let paddr = 0x8000_0000u64;
        let next_table_pte = |t: &Page| {
            crate::arch::regs::PTE_V
                | ((t.0.as_ptr() as u64 >> 12) << crate::arch::regs::PTE_PPN_SHIFT)
        };
        l2.0[(vaddr >> 30) as usize & 0x1FF] = next_table_pte(&l1);
        l1.0[(vaddr >> 21) as usize & 0x1FF] = next_table_pte(&l0);
        l0.0[(vaddr >> 12) as usize & 0x1FF] = crate::arch::regs::PTE_V
            | crate::arch::regs::PTE_R
            | crate::arch::regs::PTE_W
            | (paddr >> 12 << crate::arch::regs::PTE_PPN_SHIFT);
        let result = crate::mm::vmm::translate(l2.0.as_ptr() as u64, vaddr);
        assert_eq!(result, paddr);
    }
}

#[test]
fn test_vmm_translate_not_mapped() {
    // SAFETY: read-only walk of a stack-resident all-zero fake table.
    unsafe {
        let pt = [0u64; 512];
        let result = crate::mm::vmm::translate(pt.as_ptr() as u64, 0x2000);
        assert_eq!(result, 0);
    }
}

#[test]
fn test_vmm_translate_invalid_pte() {
    // SAFETY: read-only walk of a stack-resident fake table; the invalid
    // (PTE_V-clear) leaf terminates the walk before any dereference of a
    // fabricated address.
    unsafe {
        let mut pt = [0u64; 512];
        let vaddr = 0x1000u64;
        let l0_idx = (vaddr >> 12) & 0x1FF;
        pt[l0_idx as usize] = crate::arch::regs::PTE_R | crate::arch::regs::PTE_W;
        let result = crate::mm::vmm::translate(pt.as_ptr() as u64, vaddr);
        assert_eq!(result, 0);
    }
}

use onyx_core::errno::{Errno, KResult};

use super::Block;
use crate::mm::pmm;

/// Grow/shrink an allocation. `p` may be null (acts as `kmalloc`); a
/// `new_size` of 0 acts as `kfree`.
///
/// # Safety
///
/// `p` must be null or a live allocation from this heap (SLAB or
/// free-list). Heap initialised, interrupts disabled.
pub unsafe fn krealloc(p: *mut u8, new_size: usize) -> KResult<*mut u8> {
    // SAFETY: each branch only calls other heap entry points with the same
    // preconditions, plus one `copy_nonoverlapping` bounded by the old
    // block's size (`alloc_size`) and the new request.
    unsafe {
        if p.is_null() {
            return super::kmalloc(new_size);
        }
        if new_size == 0 {
            super::kfree(p);
            return Ok(core::ptr::null_mut());
        }
        let old_size = alloc_size(p);
        // A non-null pointer whose usable size reads as 0 is not a live
        // allocation from this heap (foreign pointer) — copying from it
        // would read out of bounds, so refuse instead.
        if old_size == 0 {
            return Err(Errno::Inval);
        }
        let copy_n = old_size.min(new_size);
        if new_size <= old_size {
            return Ok(p);
        }
        let new = super::kmalloc(new_size)?;
        core::ptr::copy_nonoverlapping(p, new, copy_n);
        super::kfree(p);
        Ok(new)
    }
}

/// Query the usable size of an allocation.
///
/// # Safety
///
/// `p` must be null or a pointer previously returned by this heap; the
/// function dereferences the block header (or slab page header) at `p`.
unsafe fn alloc_size(p: *mut u8) -> usize {
    // SAFETY: the slab path validates `SLAB_MAGIC` and bounds-checks
    // `size_idx`; the free-list path subtracts the header size from a
    // pointer that (per contract) came from `kmalloc`, so the header read
    // is in-bounds. A null `p` returns 0 without dereferencing.
    unsafe {
        if p.is_null() {
            return 0;
        }
        let page_addr = (p as usize) & !(pmm::PAGE_SIZE - 1);
        let page = page_addr as *const pmm::slab::SlabHeader;
        if (*page).magic == pmm::SLAB_MAGIC {
            let class = (*page).size_idx as usize;
            if class < pmm::SLAB_SIZES.len() {
                return pmm::SLAB_SIZES[class];
            }
            return 0;
        }
        let blk_addr = p as usize - Block::hdr_size();
        let blk = blk_addr as *const Block;
        (*blk).size
    }
}

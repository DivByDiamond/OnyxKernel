use super::{Block, G_HEAP};
use crate::mm::pmm;
use onyx_core::errno::KResult;

pub unsafe fn krealloc(p: *mut u8, new_size: usize) -> KResult<*mut u8> {
    if p.is_null() {
        return super::kmalloc(new_size);
    }
    if new_size == 0 {
        super::kfree(p);
        return Ok(core::ptr::null_mut());
    }
    let old_size = alloc_size(p);
    let copy_n = if old_size == 0 {
        new_size
    } else {
        old_size.min(new_size)
    };
    if old_size > 0 && new_size <= old_size {
        return Ok(p);
    }
    let new = super::kmalloc(new_size)?;
    core::ptr::copy_nonoverlapping(p, new, copy_n);
    super::kfree(p);
    Ok(new)
}

unsafe fn alloc_size(p: *mut u8) -> usize {
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

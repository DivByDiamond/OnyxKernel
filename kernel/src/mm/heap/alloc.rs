use super::{lock_heap, unlock_heap, Block, G_HEAP, HEAP_SIZE, MIN_BLOCK};
use crate::mm::pmm;
use onyx_core::errno::{Errno, KResult};

pub unsafe fn kmalloc(size: usize) -> KResult<*mut u8> {
    if size == 0 {
        return Err(Errno::Inval);
    }
    if size > isize::MAX as usize - 16 {
        return Err(Errno::NoMem);
    }
    lock_heap();
    let res = kmalloc_locked(size);
    unlock_heap();
    res
}

unsafe fn kmalloc_locked(size: usize) -> KResult<*mut u8> {
    if let Some(p) = pmm::slab_alloc(size) {
        (*&raw mut G_HEAP).used += size;
        return Ok(p);
    }
    let needed = (size + 15) & !15;
    let total = needed + Block::hdr_size();
    let pg = &raw const G_HEAP;
    let mut cur = (*pg).free_list;
    while !cur.is_null() {
        let blk = &mut *cur;
        if blk.free && blk.size >= total {
            if blk.size >= total + MIN_BLOCK + Block::hdr_size() {
                let new_addr = cur as usize + Block::hdr_size() + needed;
                let new_blk = new_addr as *mut Block;
                (*new_blk).size = blk.size - needed - Block::hdr_size();
                (*new_blk).free = true;
                (*new_blk).next = blk.next;
                (*new_blk).prev = cur;
                if !blk.next.is_null() {
                    (*blk.next).prev = new_blk;
                }
                blk.next = new_blk;
                blk.size = needed;
            }
            blk.free = false;
            (*&raw mut G_HEAP).used += needed;
            return Ok((cur as usize + Block::hdr_size()) as *mut u8);
        }
        cur = blk.next;
    }
    Err(Errno::NoMem)
}

pub unsafe fn kfree(p: *mut u8) {
    if p.is_null() {
        return;
    }
    lock_heap();
    kfree_locked(p);
    unlock_heap();
}

unsafe fn kfree_locked(p: *mut u8) {
    if pmm::slab_free(p) {
        return;
    }
    if (p as usize) < Block::hdr_size() || (p as usize) & 15 != 0 {
        return;
    }
    let blk_addr = p as usize - Block::hdr_size();
    let blk = blk_addr as *mut Block;
    if (*blk).size == 0 || (*blk).size > HEAP_SIZE {
        return;
    }
    (*&raw mut G_HEAP).used -= (*blk).size;
    (*blk).free = true;
    if !(*blk).next.is_null() && (*(*blk).next).free {
        let next = (*blk).next;
        (*blk).size += Block::hdr_size() + (*next).size;
        (*blk).next = (*next).next;
        if !(*blk).next.is_null() {
            (*(*blk).next).prev = blk;
        }
    }
    if !(*blk).prev.is_null() && (*(*blk).prev).free {
        let prev = (*blk).prev;
        (*prev).size += Block::hdr_size() + (*blk).size;
        (*prev).next = (*blk).next;
        if !(*prev).next.is_null() {
            (*(*prev).next).prev = prev;
        }
    }
}

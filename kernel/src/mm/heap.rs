//! Heap allocator (kmalloc/kfree) with SLAB integration.
use crate::mm::pmm;
use core::sync::atomic::{AtomicBool, Ordering};
use onyx_core::errno::{Errno, KResult};
pub const HEAP_SIZE: usize = 4 * 1024 * 1024;
pub const MIN_BLOCK: usize = 16;

static HEAP_LOCK: AtomicBool = AtomicBool::new(false);

fn lock_heap() {
    while HEAP_LOCK.swap(true, Ordering::Acquire) {}
}

fn unlock_heap() {
    HEAP_LOCK.store(false, Ordering::Release);
}

#[repr(C)]
struct Block {
    size: usize,
    free: bool,
    next: *mut Block,
    prev: *mut Block,
}
impl Block {
    const fn hdr_size() -> usize {
        core::mem::size_of::<Self>()
    }
}

struct Heap {
    #[expect(dead_code)]
    base: usize,
    #[expect(dead_code)]
    size: usize,
    used: usize,
    free_list: *mut Block,
}
static mut G_HEAP: Heap = Heap {
    base: 0,
    size: 0,
    used: 0,
    free_list: core::ptr::null_mut(),
};

pub unsafe fn init() {
    let kernel_end_pa = &crate::arch::__kernel_end as *const u8 as usize;
    let block = kernel_end_pa as *mut Block;
    (*block).size = HEAP_SIZE - Block::hdr_size();
    (*block).free = true;
    (*block).next = core::ptr::null_mut();
    (*block).prev = core::ptr::null_mut();
    let p = &raw mut G_HEAP;
    *p = Heap {
        base: kernel_end_pa,
        size: HEAP_SIZE,
        used: 0,
        free_list: block,
    };
}

pub unsafe fn kmalloc(size: usize) -> KResult<*mut u8> {
    if size == 0 {
        return Err(Errno::Inval);
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
    let blk_addr = p as usize - Block::hdr_size();
    let blk = blk_addr as *mut Block;
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

pub unsafe fn krealloc(p: *mut u8, new_size: usize) -> KResult<*mut u8> {
    if p.is_null() {
        return kmalloc(new_size);
    }
    if new_size == 0 {
        kfree(p);
        return Err(Errno::Inval);
    }
    // Bug #4 fix: previously krealloc unconditionally copied `new_size` bytes
    // from the old buffer. If new_size > old allocation size, this read past
    // the old buffer's end and leaked kernel heap data into the new buffer
    // (heap over-read). We now introspect the old allocation's actual size
    // via alloc_size() and copy min(old_size, new_size) bytes.
    let old_size = alloc_size(p);
    let copy_n = if old_size == 0 {
        new_size
    } else {
        old_size.min(new_size)
    };
    let new = kmalloc(new_size)?;
    core::ptr::copy_nonoverlapping(p, new, copy_n);
    kfree(p);
    Ok(new)
}

/// Return the usable size of the allocation at `p`, or 0 if unknown.
/// Used by krealloc to bound the copy when shrinking/growing. Looks at
/// the slab header (if the allocation came from the slab allocator) or
/// the block header (if it came from the free-list).
unsafe fn alloc_size(p: *mut u8) -> usize {
    if p.is_null() {
        return 0;
    }
    // Try slab first: the page-aligned address holds a SlabHeader with
    // SLAB_MAGIC if this is a slab allocation.
    let page_addr = (p as usize) & !(pmm::PAGE_SIZE - 1);
    let page = page_addr as *const pmm::slab::SlabHeader;
    if (*page).magic == pmm::SLAB_MAGIC {
        let class = (*page).size_idx as usize;
        if class < pmm::SLAB_SIZES.len() {
            return pmm::SLAB_SIZES[class];
        }
        return 0;
    }
    // Otherwise it's a free-list block. The Block header sits immediately
    // before the user pointer.
    let blk_addr = p as usize - Block::hdr_size();
    let blk = blk_addr as *const Block;
    // Best-effort: trust the size field. We can't easily validate without
    // a magic, but kfree_locked has the same trust assumption.
    (*blk).size
}
pub fn used() -> usize {
    unsafe { (*&raw const G_HEAP).used }
}

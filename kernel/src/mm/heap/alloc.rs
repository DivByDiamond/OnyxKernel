use onyx_core::errno::{Errno, KResult};

use super::{Block, G_HEAP, HEAP_SIZE, MIN_BLOCK, lock_heap, unlock_heap};
use crate::mm::pmm;

/// Allocate `size` bytes from the kernel heap (SLAB first, then the
/// free-list).
///
/// # Safety
///
/// The heap must be initialised (`heap::init`) and interrupts disabled
/// (spinlock invariant). On success the pointer must later be released
/// with [`kfree`] or reallocated with [`krealloc`].
pub unsafe fn kmalloc(size: usize) -> KResult<*mut u8> {
    // SAFETY: lock acquisition itself is safe; the locked body upholds the
    // `# Safety` contract above.
    unsafe {
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
}

/// Locking variant of [`kmalloc`]. Caller MUST hold the heap lock.
///
/// # Safety
///
/// Same as [`kmalloc`] plus: caller must hold `lock_heap()`. All
/// raw-pointer dereferences below walk the `G_HEAP.free_list`, whose nodes
/// were created exclusively by this module inside the lock.
unsafe fn kmalloc_locked(size: usize) -> KResult<*mut u8> {
    // SAFETY: heap lock held; free-list nodes and slab headers are only
    // ever mutated under this lock, so the pointer walk is exclusive.
    unsafe {
        if let Some(p) = pmm::slab_alloc(size) {
            G_HEAP.used += size;
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
                G_HEAP.used += needed;
                return Ok((cur as usize + Block::hdr_size()) as *mut u8);
            }
            cur = blk.next;
        }
        Err(Errno::NoMem)
    }
}

/// Free a pointer returned by [`kmalloc`]/[`krealloc`].
///
/// # Safety
///
/// `p` must be null or a live allocation from this heap; double-free or
/// freeing a foreign pointer corrupts the free-list. Heap initialised,
/// interrupts disabled.
pub unsafe fn kfree(p: *mut u8) {
    // SAFETY: lock acquisition itself is safe; the locked body upholds the
    // `# Safety` contract above.
    unsafe {
        if p.is_null() {
            return;
        }
        lock_heap();
        kfree_locked(p);
        unlock_heap();
    }
}

/// Locking variant of [`kfree`]. Caller MUST hold the heap lock.
///
/// # Safety
///
/// Same as [`kfree`] plus: caller must hold `lock_heap()`. The block
/// pointer arithmetic is guarded by the alignment/size sanity checks, and
/// coalescing only touches list nodes created by this module.
unsafe fn kfree_locked(p: *mut u8) {
    // SAFETY: heap lock held; `slab_free` validates the magic, and for
    // free-list blocks the alignment/size checks below gate every Block
    // dereference and coalescing step.
    unsafe {
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
        G_HEAP.used -= (*blk).size;
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
}

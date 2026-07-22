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

mod alloc;
mod realloc;

pub use alloc::*;
pub use realloc::*;

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

pub fn used() -> usize {
    unsafe { (*&raw const G_HEAP).used }
}

use crate::sync::SpinLock;
pub const HEAP_SIZE: usize = 4 * 1024 * 1024;
pub const MIN_BLOCK: usize = 16;

// Unified SpinLock primitive (audit fix): this used to be a bare AtomicBool
// with NO backoff — under SMP contention it re-issued an LL/SC swap in a tight
// loop, hammering the holder's cache line.
static HEAP_LOCK: SpinLock = SpinLock::new();

fn lock_heap() {
    HEAP_LOCK.lock();
}

fn unlock_heap() {
    HEAP_LOCK.unlock();
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
    used: usize,
    free_list: *mut Block,
}
static mut G_HEAP: Heap = Heap {
    used: 0,
    free_list: core::ptr::null_mut(),
};

mod alloc;
mod realloc;

pub use alloc::*;

pub use realloc::*;

/// Initialise the bump/free-list kernel heap over the `HEAP_SIZE` region
/// that starts at the linker-provided `__kernel_end` symbol.
///
/// # Safety
///
/// Must be called exactly once during early boot, single-threaded, before
/// any allocation is made. The caller guarantees that `[__kernel_end,
/// __kernel_end + HEAP_SIZE)` is writable physical memory (reserved by the
/// PMM/linker script) and that no other code aliases it yet.
pub unsafe fn init() {
    // SAFETY: single-threaded early boot per the `# Safety` contract; the
    // Block write targets reserved physical memory past `__kernel_end`.
    unsafe {
        let kernel_end_pa = &crate::arch::__kernel_end as *const u8 as usize;
        let block = kernel_end_pa as *mut Block;
        (*block).size = HEAP_SIZE - Block::hdr_size();
        (*block).free = true;
        (*block).next = core::ptr::null_mut();
        (*block).prev = core::ptr::null_mut();
        let p = &raw mut G_HEAP;
        // SAFETY: `G_HEAP` is only ever written here during early boot,
        // before any other hart or allocator call can observe it.
        *p = Heap {
            used: 0,
            free_list: block,
        };
    }
}

pub fn used() -> usize {
    // SAFETY: reading a `usize` counter; torn reads are impossible on the
    // supported targets and callers accept the (slightly stale) value.
    unsafe { G_HEAP.used }
}

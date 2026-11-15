//! Per-executable 16 KiB bump allocator, explicitly installed by each binary.
//!
//! Shared library modules never install a global allocator. Auth uses fixed
//! buffers; the Lua VM allocates from this arena. Deallocation remains a no-op,
//! preserving the existing executables' allocator behavior and capacity.

use core::alloc::{GlobalAlloc, Layout};
use core::ptr;
use core::sync::atomic::{AtomicBool, Ordering};

const ARENA_SIZE: usize = 16 * 1024;

struct BumpAlloc;

static BUSY: AtomicBool = AtomicBool::new(false);
static mut ARENA: [u8; ARENA_SIZE] = [0u8; ARENA_SIZE];
static mut NEXT: usize = 0;

impl BumpAlloc {
    fn alloc_inner(&self, layout: Layout) -> *mut u8 {
        // SAFETY: BUSY serializes arena/NEXT access; returned ranges stay in ARENA.
        unsafe {
            let base = ptr::addr_of_mut!(ARENA) as usize;
            let start = (base + NEXT).next_multiple_of(layout.align().max(1));
            let end = match start.checked_add(layout.size()) {
                Some(e) => e,
                None => return ptr::null_mut(),
            };
            if end > base + ARENA_SIZE {
                return ptr::null_mut();
            }
            NEXT = end - base;
            start as *mut u8
        }
    }
}

// SAFETY: BUSY serializes allocation; aligned disjoint ranges come from a static arena.
unsafe impl GlobalAlloc for BumpAlloc {
    /// # Safety
    /// `layout` must meet GlobalAlloc's nonzero-size allocation contract.
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        while BUSY.swap(true, Ordering::Acquire) {
            core::hint::spin_loop();
        }
        let p = self.alloc_inner(layout);
        BUSY.store(false, Ordering::Release);
        p
    }

    /// # Safety
    /// The pointer/layout pair must denote a live allocation from this allocator.
    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
}

#[global_allocator]
static ALLOCATOR: BumpAlloc = BumpAlloc;

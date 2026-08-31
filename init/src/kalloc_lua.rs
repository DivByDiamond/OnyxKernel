//! Minimal global allocator for the auth-using init binaries.
//!
//! The pure crypto/KDF code now lives in `onyx_core`, whose crate root
//! declares `extern crate alloc`; linking that crate therefore requires
//! every consumer binary to define a `#[global_allocator]`. Nothing in the
//! init userspace actually performs a heap allocation today, so this bump
//! allocator exists purely to satisfy the linker contract — it is expected
//! to stay untouched at runtime. If allocation ever happens and the tiny
//! arena is exhausted, requests fail cleanly (`null`) instead of panicking.
//!
//! Design notes:
//! - Fixed static arena, single-word bump pointer, spin bit for safety
//!   under the (currently impossible) concurrent case.
//! - `dealloc` is a no-op: bump allocation never reclaims. Acceptable for
//!   an allocator that must never be exercised.

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

unsafe impl GlobalAlloc for BumpAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        while BUSY.swap(true, Ordering::Acquire) {
            core::hint::spin_loop();
        }
        let p = self.alloc_inner(layout);
        BUSY.store(false, Ordering::Release);
        p
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
}

#[global_allocator]
static ALLOCATOR: BumpAlloc = BumpAlloc;

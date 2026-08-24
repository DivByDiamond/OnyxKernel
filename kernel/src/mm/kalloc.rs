//! Global allocator for kernel — bridges alloc crate to heap::kmalloc/kfree.

use crate::mm::heap;
use core::alloc::{GlobalAlloc, Layout};

struct KernelAlloc;

// SAFETY: `alloc` returns null on failure/OOM (never a dangling pointer)
// and `dealloc` releases exactly the pointer handed out for `layout`;
// alignment > 16 is served by an over-allocation whose original base is
// stored just before the payload, so dealloc can recover it.
unsafe impl GlobalAlloc for KernelAlloc {
    /// # Safety
    /// Per `GlobalAlloc`: `layout` must have non-zero size and the returned
    /// pointer (if non-null) must be deallocated with this allocator.
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: contract upheld per the impl note — kmalloc either
        // succeeds with a valid pointer or errors (mapped to null), and
        // the over-allocation keeps the base address 8 bytes below an
        // aligned payload.
        unsafe {
            let size = layout.size();
            let align = layout.align();
            if align <= 16 {
                match heap::kmalloc(size) {
                    Ok(p) => p,
                    Err(_) => core::ptr::null_mut(),
                }
            } else {
                let total = size + align + core::mem::size_of::<usize>();
                match heap::kmalloc(total) {
                    Ok(p) => {
                        let addr = p as usize;
                        let payload =
                            (addr + core::mem::size_of::<usize>() + align - 1) & !(align - 1);
                        let store = (payload - core::mem::size_of::<usize>()) as *mut usize;
                        store.write(addr);
                        payload as *mut u8
                    }
                    Err(_) => core::ptr::null_mut(),
                }
            }
        }
    }

    /// # Safety
    /// Per `GlobalAlloc`: `ptr` must denote a block of memory currently
    /// allocated by this allocator with a layout equal to `layout`.
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` was returned by `alloc` above for this `layout`;
        // for aligned allocations the stored base is read back and freed,
        // matching how the pointer was produced.
        unsafe {
            if layout.align() <= 16 {
                heap::kfree(ptr);
            } else {
                let orig_ptr = ((ptr as usize) - core::mem::size_of::<usize>()) as *const usize;
                heap::kfree(orig_ptr.read() as *mut u8);
            }
        }
    }
}

#[cfg(not(test))]
#[global_allocator]
static ALLOCATOR: KernelAlloc = KernelAlloc;

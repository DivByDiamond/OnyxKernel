//! Global allocator for kernel — bridges alloc crate to heap::kmalloc/kfree.

use crate::mm::heap;
use core::alloc::{GlobalAlloc, Layout};

struct KernelAlloc;

unsafe impl GlobalAlloc for KernelAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
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
                    let payload = (addr + core::mem::size_of::<usize>() + align - 1) & !(align - 1);
                    let store = (payload - core::mem::size_of::<usize>()) as *mut usize;
                    store.write(addr);
                    payload as *mut u8
                }
                Err(_) => core::ptr::null_mut(),
            }
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if layout.align() <= 16 {
            heap::kfree(ptr);
        } else {
            let orig_ptr = ((ptr as usize) - core::mem::size_of::<usize>()) as *const usize;
            heap::kfree(orig_ptr.read() as *mut u8);
        }
    }
}

#[global_allocator]
static ALLOCATOR: KernelAlloc = KernelAlloc;

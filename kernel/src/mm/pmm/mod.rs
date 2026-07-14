//! PMM — Physical Memory Manager with buddy+SLAB hybrid.
//!
//! This is the directory root. It owns the `Pmm` struct, the global `G_PMM`
//! static, the `init` entry point, and the `free_pages` counter. Bitmap
//! operations (alloc/free) live in `bitmap.rs`; SLAB operations live in
//! `slab.rs`.
use crate::arch::__kernel_end;
use core::hint::spin_loop;
use core::ptr;
use core::sync::atomic::{AtomicBool, Ordering};

pub const PAGE_SIZE: usize = 4096;
pub const KERNEL_HEAP_RESERVE: usize = 4 * 1024 * 1024;

pub(super) const SLAB_SIZES: [usize; 3] = [64, 256, 1024];
pub(super) const SLAB_MAGIC: u32 = 0x534C_4142;

pub(super) struct Pmm {
    pub(super) base: usize,
    pub(super) total_pages: usize,
    pub(super) free_pages: usize,
    pub(super) bitmap: *mut u8,
    #[expect(dead_code)]
    pub(super) bitmap_bytes: usize,
    pub(super) slab_heads: [*mut slab::SlabHeader; SLAB_SIZES.len()],
}

pub(super) static mut G_PMM: Pmm = Pmm {
    base: 0,
    total_pages: 0,
    free_pages: 0,
    bitmap: ptr::null_mut(),
    bitmap_bytes: 0,
    slab_heads: [ptr::null_mut(); SLAB_SIZES.len()],
};

/// Global PMM spinlock (Bug #1 fix). All bitmap and slab free-list mutations
/// go through this lock, preventing the SMP race where two harts simultaneously
/// read the same bitmap bit as free, both set it, and return the same PA —
/// leading to double-mapping, UAF, and silent memory corruption.
///
/// Callers must use `pmm_lock` / `pmm_unlock` around any sequence that reads
/// and mutates `G_PMM` fields, the bitmap, or the slab free-lists. Internal
/// `_unlocked` variants exist for call sites that already hold the lock.
pub(super) static G_PMM_LOCK: AtomicBool = AtomicBool::new(false);

#[inline]
pub(super) unsafe fn pmm_lock() {
    while G_PMM_LOCK.swap(true, Ordering::Acquire) {
        while G_PMM_LOCK.load(Ordering::Relaxed) {
            spin_loop();
        }
    }
}

#[inline]
pub(super) unsafe fn pmm_unlock() {
    G_PMM_LOCK.store(false, Ordering::Release);
}

pub unsafe fn init(dram_base: u64, dram_size: u64) {
    let kernel_end_pa = &__kernel_end as *const u8 as usize;
    let heap_end_pa = kernel_end_pa + KERNEL_HEAP_RESERVE;
    let managed_base = core::cmp::max(heap_end_pa, dram_base as usize);
    let managed_base = (managed_base + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
    let managed_end = (dram_base + dram_size) as usize;
    let managed_size = managed_end.saturating_sub(managed_base);
    let pages = managed_size / PAGE_SIZE;
    let bitmap_bytes = pages.div_ceil(8);
    let bitmap_pages = bitmap_bytes.div_ceil(PAGE_SIZE);
    let bitmap = managed_base as *mut u8;
    ptr::write_bytes(bitmap, 0, bitmap_bytes);
    let data_base = managed_base + bitmap_pages * PAGE_SIZE;
    let data_pages = pages.saturating_sub(bitmap_pages);
    let p = &raw mut G_PMM;
    *p = Pmm {
        base: data_base,
        total_pages: data_pages,
        free_pages: data_pages,
        bitmap,
        bitmap_bytes,
        slab_heads: [ptr::null_mut(); SLAB_SIZES.len()],
    };
    for i in 0..bitmap_pages {
        bitmap::bm_set(i);
    }
    // Bug (mm SERIOUS #12): also mark every page in the [data_base, data_base +
    // data_pages) range that overlaps the bitmap pages themselves. The bitmap
    // lives at `managed_base .. managed_base + bitmap_pages*PAGE_SIZE`, which
    // is INSIDE the [data_base, data_base+data_pages) range we hand out to
    // callers. Without marking those bitmap-occupied pages as used in the
    // bitmap itself, pmm::alloc() could hand out a page that overlaps the
    // bitmap buffer — silently corrupting the allocator state.
    //
    // We mark the bitmap pages by their absolute bit index. The bitmap's
    // data_base offset is `bitmap_pages * PAGE_SIZE` from `base` (the
    // physical address of bit 0). So bit i in the bitmap corresponds to
    // physical address `base + i*PAGE_SIZE`, and bitmap pages occupy bits
    // `(managed_base - base) / PAGE_SIZE ..` for bitmap_pages entries.
    //
    // In the current layout `managed_base == data_base` (we place the
    // bitmap at the very start of the managed region), so the loop above
    // already marks the right bits. We additionally mark any page that
    // falls between the original kernel_end and managed_base — those are
    // reserved for kernel BSS / heap and must never be handed out.
    let kernel_end_pa = &__kernel_end as *const u8 as usize;
    let reserved_end = managed_base; // everything below managed_base is reserved
    if kernel_end_pa < reserved_end {
        let start_bit = (kernel_end_pa.saturating_sub((*p).base)) / PAGE_SIZE;
        let end_bit = (reserved_end.saturating_sub((*p).base)) / PAGE_SIZE;
        for i in start_bit..end_bit {
            if i < (*p).total_pages && !bitmap::bm_get(i) {
                bitmap::bm_set(i);
            }
        }
    }
    crate::srv::klog::emit(
        crate::srv::klog::Level::Inf,
        "pmm",
        "dram 0x%x + 0x%x, managed base=0x%x pages=%d free=%d",
        &[
            onyx_core::fmt::Arg::from(dram_base),
            onyx_core::fmt::Arg::from(dram_size),
            onyx_core::fmt::Arg::from(data_base as u64),
            onyx_core::fmt::Arg::from(data_pages),
            onyx_core::fmt::Arg::from(data_pages),
        ],
    );
}

pub fn free_pages() -> usize {
    unsafe { (*(&raw const G_PMM)).free_pages }
}

pub fn total_pages() -> usize {
    unsafe { (*(&raw const G_PMM)).total_pages }
}

pub fn is_managed(paddr: u64) -> bool {
    unsafe {
        let p = &raw const G_PMM;
        let pa = paddr as usize;
        // Bug (mm MINOR #2): require page alignment. The previous code
        // accepted any address in [base, end) — a non-page-aligned PA
        // would later compute a bogus bit index in pa_to_idx and corrupt
        // the bitmap. Refuse anything that isn't page-aligned.
        if pa & (PAGE_SIZE - 1) != 0 {
            return false;
        }
        let end = (*p).base + (*p).total_pages * PAGE_SIZE;
        pa >= (*p).base && pa < end
    }
}

pub mod bitmap;
pub mod slab;

pub use bitmap::{alloc, alloc_n, alloc_zero, free};
pub use slab::{slab_alloc, slab_free};

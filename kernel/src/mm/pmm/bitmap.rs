//! Bitmap-based page allocator — `bm_get`/`bm_set`/`bm_clr` primitives plus
//! the public `alloc`/`alloc_n`/`free`/`alloc_zero` entry points.
use super::{G_PMM, PAGE_SIZE};
use core::ptr;
use onyx_core::errno::{Errno, KResult};

pub(super) unsafe fn bm_get(bit: usize) -> bool {
    let p = &raw const G_PMM;
    let bmp = (*p).bitmap;
    *bmp.add(bit / 8) & (1 << (bit % 8)) != 0
}
pub(super) unsafe fn bm_set(bit: usize) {
    let p = &raw const G_PMM;
    let bmp = (*p).bitmap;
    *bmp.add(bit / 8) |= 1 << (bit % 8);
    (*(&raw mut G_PMM)).free_pages -= 1;
}
pub(super) unsafe fn bm_clr(bit: usize) {
    let p = &raw const G_PMM;
    let bmp = (*p).bitmap;
    *bmp.add(bit / 8) &= !(1 << (bit % 8));
    (*(&raw mut G_PMM)).free_pages += 1;
}
fn pa_to_idx(pa: usize) -> usize {
    unsafe {
        let p = &raw const G_PMM;
        (pa - (*p).base) / PAGE_SIZE
    }
}
fn idx_to_pa(idx: usize) -> usize {
    unsafe {
        let p = &raw const G_PMM;
        (*p).base + idx * PAGE_SIZE
    }
}

pub unsafe fn alloc() -> KResult<u64> {
    super::pmm_lock();
    let r = alloc_unlocked();
    super::pmm_unlock();
    r
}

/// Internal alloc without locking. Caller MUST hold `pmm_lock()`.
pub(super) unsafe fn alloc_unlocked() -> KResult<u64> {
    let p = &raw const G_PMM;
    let n = (*p).total_pages;
    let mut i = 0;
    while i < n {
        if !bm_get(i) {
            bm_set(i);
            let pa = idx_to_pa(i);
            ptr::write_bytes(pa as *mut u8, 0, PAGE_SIZE);
            return Ok(pa as u64);
        }
        i += 1;
    }
    Err(Errno::NoMem)
}

pub unsafe fn alloc_n(n: usize) -> KResult<u64> {
    super::pmm_lock();
    let r = alloc_n_unlocked(n);
    super::pmm_unlock();
    r
}

/// Internal alloc_n without locking. Caller MUST hold `pmm_lock()`.
pub(super) unsafe fn alloc_n_unlocked(n: usize) -> KResult<u64> {
    if n == 0 {
        return Err(Errno::Inval);
    }
    let p = &raw const G_PMM;
    let total = (*p).total_pages;
    let mut run = 0usize;
    let mut start = 0usize;
    let mut i = 0;
    while i < total {
        if !bm_get(i) {
            if run == 0 {
                start = i;
            }
            run += 1;
            if run == n {
                for k in start..start + n {
                    bm_set(k);
                }
                return Ok(idx_to_pa(start) as u64);
            }
        } else {
            run = 0;
        }
        i += 1;
    }
    Err(Errno::NoMem)
}

pub unsafe fn free(pa: u64) {
    super::pmm_lock();
    free_unlocked(pa);
    super::pmm_unlock();
}

/// Internal free without locking. Caller MUST hold `pmm_lock()`.
pub(super) unsafe fn free_unlocked(pa: u64) {
    let idx = pa_to_idx(pa as usize);
    if idx < unsafe { (*(&raw const G_PMM)).total_pages } {
        if bm_get(idx) {
            bm_clr(idx);
        }
    }
}

pub unsafe fn alloc_zero() -> KResult<u64> {
    // alloc() already acquires pmm_lock; no extra locking needed here.
    alloc()
}

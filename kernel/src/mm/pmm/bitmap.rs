//! Bitmap-based page allocator — `bm_get`/`bm_set`/`bm_clr` primitives plus
//! the public `alloc`/`alloc_n`/`free`/`alloc_zero` entry points.
use core::ptr;

use onyx_core::errno::{Errno, KResult};

use super::{G_PMM, PAGE_SIZE};

/// Read bitmap bit `bit` (0 = first managed page).
///
/// # Safety
///
/// `bit` must be `< G_PMM.total_pages` and `pmm::init` must have completed.
pub(super) unsafe fn bm_get(bit: usize) -> bool {
    // SAFETY: `G_PMM.bitmap` points at `bitmap_bytes` bytes allocated by
    // `pmm::init`; `bit / 8` is in range by the caller contract above.
    unsafe {
        let p = &raw const G_PMM;
        let bmp = (*p).bitmap;
        *bmp.add(bit / 8) & (1 << (bit % 8)) != 0
    }
}
/// Mark page `bit` as used and decrement the free-page counter.
///
/// # Safety
///
/// Same contract as [`bm_get`]; additionally the caller must hold
/// `pmm_lock()` (or be in single-threaded early init) since this mutates
/// shared allocator state.
pub(super) unsafe fn bm_set(bit: usize) {
    // SAFETY: see `bm_get`; mutation is serialised by the PMM lock per the
    // caller contract.
    unsafe {
        let p = &raw const G_PMM;
        let bmp = (*p).bitmap;
        *bmp.add(bit / 8) |= 1 << (bit % 8);
        G_PMM.free_pages -= 1;
    }
}
/// Mark page `bit` as free and increment the free-page counter.
///
/// # Safety
///
/// Same contract as [`bm_set`] (valid `bit`, lock held).
pub(super) unsafe fn bm_clr(bit: usize) {
    // SAFETY: see `bm_get`; mutation is serialised by the PMM lock per the
    // caller contract.
    unsafe {
        let p = &raw const G_PMM;
        let bmp = (*p).bitmap;
        *bmp.add(bit / 8) &= !(1 << (bit % 8));
        G_PMM.free_pages += 1;
    }
}
fn pa_to_idx(pa: usize) -> usize {
    // SAFETY: read-only access to fields initialised by `pmm::init`.
    unsafe {
        let p = &raw const G_PMM;
        (pa - (*p).base) / PAGE_SIZE
    }
}
fn idx_to_pa(idx: usize) -> usize {
    // SAFETY: read-only access to fields initialised by `pmm::init`.
    unsafe {
        let p = &raw const G_PMM;
        (*p).base + idx * PAGE_SIZE
    }
}

/// Allocate a single zeroed physical page.
///
/// # Safety
///
/// `pmm::init` must have completed. Interrupts must be disabled (spinlock
/// invariant).
pub unsafe fn alloc() -> KResult<u64> {
    // SAFETY: lock acquisition is safe; `alloc_unlocked` upholds the
    // `# Safety` contract above under the PMM lock (`G_PMM_LOCK`).
    unsafe {
        super::pmm_lock();
        let r = alloc_unlocked();
        super::pmm_unlock();
        r
    }
}

/// Internal alloc without locking. Caller MUST hold `pmm_lock()`.
///
/// # Safety
///
/// Same as [`alloc`] plus: the caller must hold `pmm_lock()` so the
/// bitmap scan-and-set is atomic with respect to other allocators.
pub(super) unsafe fn alloc_unlocked() -> KResult<u64> {
    // SAFETY: raw pointer reads of `G_PMM` fields and a write of
    // PAGE_SIZE zero bytes to the returned frame; the index is bounded by
    // `total_pages` and the frame is PMM-managed RAM.
    unsafe {
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
}

/// Allocate `n` contiguous zeroed physical pages.
///
/// # Safety
///
/// Same as [`alloc`]: PMM initialised, interrupts disabled.
pub unsafe fn alloc_n(n: usize) -> KResult<u64> {
    // SAFETY: lock acquisition is safe; `alloc_n_unlocked` upholds the
    // `# Safety` contract above under the PMM lock (`G_PMM_LOCK`).
    unsafe {
        super::pmm_lock();
        let r = alloc_n_unlocked(n);
        super::pmm_unlock();
        r
    }
}

/// Internal alloc_n without locking. Caller MUST hold `pmm_lock()`.
///
/// # Safety
///
/// Same as [`alloc_n`] plus: caller must hold `pmm_lock()`.
pub(super) unsafe fn alloc_n_unlocked(n: usize) -> KResult<u64> {
    // SAFETY: `G_PMM` field reads and a zeroing write of `n * PAGE_SIZE`
    // bytes into PMM-managed RAM; all indices bounded by `total_pages`.
    unsafe {
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
                    let pa = idx_to_pa(start);
                    // Bug (mm MINOR #1): zero every page in the run. The
                    // previous code returned uninitialized physical memory —
                    // callers (e.g. large alloc_zero requests) expected zeroed
                    // pages and would read stale kernel data.
                    ptr::write_bytes(pa as *mut u8, 0, n * PAGE_SIZE);
                    return Ok(pa as u64);
                }
            } else {
                run = 0;
            }
            i += 1;
        }
        Err(Errno::NoMem)
    }
}

/// Return one previously allocated page to the free pool.
///
/// # Safety
///
/// `pa` must be a page-aligned physical address that was returned by a
/// previous allocation and is not in use; PMM must be initialised and
/// interrupts disabled.
pub unsafe fn free(pa: u64) {
    // SAFETY: lock acquisition is safe; `free_unlocked` upholds the
    // `# Safety` contract above under the PMM lock (`G_PMM_LOCK`).
    unsafe {
        super::pmm_lock();
        free_unlocked(pa);
        super::pmm_unlock();
    }
}

/// Internal free without locking. Caller MUST hold `pmm_lock()`.
///
/// # Safety
///
/// Same as [`free`] plus: caller must hold `pmm_lock()`.
pub(super) unsafe fn free_unlocked(pa: u64) {
    // SAFETY: the address is page-aligned and at or above `G_PMM.base`
    // before `pa_to_idx` runs, so the index computation cannot underflow;
    // the index is range-checked against `total_pages` before the bit is
    // cleared.
    unsafe {
        let pa = pa as usize;
        // Refuse non-page-aligned or below-base addresses: pa_to_idx would
        // either round down onto a neighbouring live page or underflow.
        if pa & (PAGE_SIZE - 1) != 0 || pa < G_PMM.base {
            return;
        }
        let idx = pa_to_idx(pa);
        if idx < G_PMM.total_pages && bm_get(idx) {
            bm_clr(idx);
        }
    }
}

/// Allocate a single zeroed physical page (same contract as [`alloc`]).
///
/// # Safety
///
/// Same as [`alloc`].
pub unsafe fn alloc_zero() -> KResult<u64> {
    // SAFETY: `alloc` self-locks and upholds its own `# Safety` contract;
    // this wrapper touches no additional raw state.
    unsafe {
        // alloc() already acquires pmm_lock; no extra locking needed here.
        alloc()
    }
}

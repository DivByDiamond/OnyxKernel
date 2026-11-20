//! The OHCI DMA pool: the static 4-KiB block the driver hands EDs and
//! TDs out of. The controller DMAs these structures directly (the pool
//! address is treated as physical — identity-mapped at boot), so every
//! helper here hands out pool *indices*, not pointers, and validates
//! bounds on each access.

use onyx_core::errno::{Errno, KResult};

const OHCI_ED_SIZE: usize = 16;
const OHCI_TD_SIZE: usize = 16;
const OHCI_DMA_POOL_SIZE: usize = 4096;
pub(super) const MAX_OHCI_ED: usize = 32;
pub(super) const MAX_OHCI_TD: usize = 64;

// OHCI data structures (spec 1.0a §4.2/§4.3; 16-byte aligned).
#[repr(C, align(16))]
pub(super) struct OhciED {
    pub(super) control: u32,
    pub(super) tail_td: u32,
    pub(super) head_td: u32,
    pub(super) next_ed: u32,
}

#[repr(C, align(16))]
pub(super) struct OhciTD {
    pub(super) control: u32,
    pub(super) cbp: u32,
    pub(super) next_td: u32,
    pub(super) be: u32,
}

#[repr(C, align(4096))]
struct OhciDmaPool {
    data: [u8; OHCI_DMA_POOL_SIZE],
}
static mut G_OHCI_DMA: OhciDmaPool = OhciDmaPool {
    data: [0; OHCI_DMA_POOL_SIZE],
};
static mut G_OHCI_DMA_USED: usize = 0;
static mut G_OHCI_ED_COUNT: usize = 0;
static mut G_OHCI_TD_COUNT: usize = 0;
static mut G_OHCI_ED_OFFSETS: [usize; MAX_OHCI_ED] = [0; MAX_OHCI_ED];
static mut G_OHCI_TD_OFFSETS: [usize; MAX_OHCI_TD] = [0; MAX_OHCI_TD];

/// # Safety
///
/// `off` must be an offset previously returned by `ohci_alloc_dma` (bounded
/// by `OHCI_DMA_POOL_SIZE`) so pool_va + off stays inside the pool.
unsafe fn ohci_pool_phys(off: usize) -> u32 {
    let pool_va = &raw const G_OHCI_DMA as usize;
    (pool_va + off) as u32
}

/// # Safety
///
/// Call from the driver's single-threaded USB path with SIE=0 (see
/// `crate::sync`); the aligned request must fit `OHCI_DMA_POOL_SIZE`
/// (checked below).
unsafe fn ohci_alloc_dma(size: usize) -> KResult<usize> {
    // SAFETY: G_OHCI_DMA_USED is only mutated here under the single-threaded (SIE=0) contract; bounds checked against OHCI_DMA_POOL_SIZE and write_bytes stays inside the aligned pool.
    unsafe {
        let aligned = (size + 15) & !15;
        let off = G_OHCI_DMA_USED;
        if off + aligned > OHCI_DMA_POOL_SIZE {
            return Err(Errno::NoMem);
        }
        G_OHCI_DMA_USED = off + aligned;
        ::core::ptr::write_bytes(G_OHCI_DMA.data.as_mut_ptr().add(off), 0, aligned);
        Ok(off)
    }
}

/// # Safety
///
/// Call from the driver's single-threaded USB path with SIE=0 (see
/// `crate::sync`); ED table state is only mutated through this helper.
pub(super) unsafe fn ohci_alloc_ed() -> KResult<usize> {
    // SAFETY: G_OHCI_ED_COUNT is bounded by the MAX_OHCI_ED check and mutated only here and in ohci_release_ed (SIE=0 per crate::sync); ohci_alloc_dma's offset is bounds-checked.
    unsafe {
        if G_OHCI_ED_COUNT >= MAX_OHCI_ED {
            return Err(Errno::NoMem);
        }
        let off = ohci_alloc_dma(OHCI_ED_SIZE)?;
        let idx = G_OHCI_ED_COUNT;
        G_OHCI_ED_OFFSETS[idx] = off;
        G_OHCI_ED_COUNT += 1;
        Ok(idx)
    }
}

/// # Safety
///
/// Call from the driver's single-threaded USB path with SIE=0 (see
/// `crate::sync`); TD table state is only mutated through this helper.
pub(super) unsafe fn ohci_alloc_td() -> KResult<usize> {
    // SAFETY: G_OHCI_TD_COUNT is bounded by the MAX_OHCI_TD check and mutated only here and in ohci_release_td (SIE=0 per crate::sync); ohci_alloc_dma's offset is bounds-checked.
    unsafe {
        if G_OHCI_TD_COUNT >= MAX_OHCI_TD {
            return Err(Errno::NoMem);
        }
        let off = ohci_alloc_dma(OHCI_TD_SIZE)?;
        let idx = G_OHCI_TD_COUNT;
        G_OHCI_TD_OFFSETS[idx] = off;
        G_OHCI_TD_COUNT += 1;
        Ok(idx)
    }
}

/// LIFO release of the most recently taken ED index — the error-path
/// counterpart of `ohci_alloc_ed` (ruling 3: give back every resource
/// an error path took).
///
/// # Safety
///
/// Call from the driver's single-threaded USB path with SIE=0 (see
/// `crate::sync`), immediately after the matching alloc, in exact
/// reverse order, and only while the ED is still unpublished (never
/// linked into a controller list). The pool's bump offset stays
/// consumed — only the table slot becomes reusable.
pub(super) unsafe fn ohci_release_ed() {
    // SAFETY: G_OHCI_ED_COUNT is mutated only on the single-owner path; saturating_sub makes misuse a no-op rather than a wrap.
    unsafe {
        G_OHCI_ED_COUNT = G_OHCI_ED_COUNT.saturating_sub(1);
    }
}

/// LIFO release of the most recently taken TD index — see
/// [`ohci_release_ed`] for the exact contract.
///
/// # Safety
///
/// Same contract as [`ohci_release_ed`].
pub(super) unsafe fn ohci_release_td() {
    // SAFETY: same single-owner discipline as ohci_release_ed.
    unsafe {
        G_OHCI_TD_COUNT = G_OHCI_TD_COUNT.saturating_sub(1);
    }
}

/// # Safety
///
/// `idx` must be an ED index previously returned by `ohci_alloc_ed` (its
/// pool offset recorded in `G_OHCI_ED_OFFSETS`).
pub(super) unsafe fn ohci_ed_ptr(idx: usize) -> *mut OhciED {
    // SAFETY: idx from ohci_alloc_ed means G_OHCI_ED_OFFSETS[idx] is a checked in-pool offset; the cast stays inside the 4096-byte aligned pool.
    unsafe { G_OHCI_DMA.data.as_mut_ptr().add(G_OHCI_ED_OFFSETS[idx]) as *mut OhciED }
}

/// # Safety
///
/// `idx` must be a TD index previously returned by `ohci_alloc_td` (its
/// pool offset recorded in `G_OHCI_TD_OFFSETS`).
pub(super) unsafe fn ohci_td_ptr(idx: usize) -> *mut OhciTD {
    // SAFETY: idx from ohci_alloc_td means G_OHCI_TD_OFFSETS[idx] is a checked in-pool offset; the cast stays inside the 4096-byte aligned pool.
    unsafe { G_OHCI_DMA.data.as_mut_ptr().add(G_OHCI_TD_OFFSETS[idx]) as *mut OhciTD }
}

/// # Safety
///
/// `idx` must be an ED index previously returned by `ohci_alloc_ed`.
pub(super) unsafe fn ohci_ed_phys(idx: usize) -> u32 {
    // SAFETY: idx from ohci_alloc_ed means the offset is a checked in-pool offset; the pool base is the static G_OHCI_DMA address.
    unsafe { ohci_pool_phys(G_OHCI_ED_OFFSETS[idx]) }
}

/// # Safety
///
/// `idx` must be a TD index previously returned by `ohci_alloc_td`.
pub(super) unsafe fn ohci_td_phys(idx: usize) -> u32 {
    // SAFETY: idx from ohci_alloc_td means the offset is a checked in-pool offset; the pool base is the static G_OHCI_DMA address.
    unsafe { ohci_pool_phys(G_OHCI_TD_OFFSETS[idx]) }
}

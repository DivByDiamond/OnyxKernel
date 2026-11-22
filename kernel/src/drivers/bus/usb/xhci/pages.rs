//! Page-ownership ledger for xHCI per-device DMA memory.
//!
//! Every page a device's lifetime needs (device context, transfer
//! rings, report buffers) is recorded here the moment it is allocated,
//! so ANY error path — and the device release — frees exactly what was
//! taken, no more and no less. The allocator and the free routine are
//! injected: the driver passes the PMM, the host tests pass counters.

use onyx_core::errno::{Errno, KResult};

/// Pages one device may own: device context, EP0 ring, interrupt ring
/// and report-buffer page, with headroom for a future bulk pair.
pub(crate) const DEV_PAGES: usize = 8;

/// Fixed-capacity ledger of owned page addresses.
pub(crate) struct PageSet {
    pages: [u64; DEV_PAGES],
    len: usize,
}

impl PageSet {
    pub(crate) const fn new() -> Self {
        PageSet {
            pages: [0; DEV_PAGES],
            len: 0,
        }
    }

    /// Pages currently owned — a checkpoint for [`Self::release_from`].
    pub(crate) fn len(&self) -> usize {
        self.len
    }

    /// Allocate one page through `alloc` and record it. A full ledger
    /// frees the fresh page through `free` and fails with `NoMem` —
    /// nothing is ever allocated unrecorded.
    pub(crate) fn take(
        &mut self,
        alloc: impl FnOnce() -> KResult<u64>,
        free: impl FnOnce(u64),
    ) -> KResult<u64> {
        let pa = alloc()?;
        if self.len >= DEV_PAGES {
            free(pa);
            return Err(Errno::NoMem);
        }
        self.pages[self.len] = pa;
        self.len += 1;
        Ok(pa)
    }

    /// Free every page recorded at or after checkpoint `mark` (newest
    /// first) — the unwind of one failed multi-page step.
    pub(crate) fn release_from(&mut self, mark: usize, mut free: impl FnMut(u64)) {
        while self.len > mark {
            self.len -= 1;
            free(self.pages[self.len]);
            self.pages[self.len] = 0;
        }
    }

    /// Free everything the device owns.
    pub(crate) fn release_all(&mut self, free: impl FnMut(u64)) {
        self.release_from(0, free);
    }
}

/// Run `f` on one temporary page and free it on EVERY path: input
/// contexts are consumed by the controller once the command completes,
/// so they never outlive the call.
pub(crate) fn with_scratch_page<T>(
    alloc: impl FnOnce() -> KResult<u64>,
    free: impl FnOnce(u64),
    f: impl FnOnce(u64) -> KResult<T>,
) -> KResult<T> {
    let pa = alloc()?;
    let r = f(pa);
    free(pa);
    r
}

#[cfg(test)]
mod tests;

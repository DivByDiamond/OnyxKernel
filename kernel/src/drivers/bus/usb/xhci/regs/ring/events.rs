//! Event ring allocation and consumer cursor.
use super::{TRB_C, Trb};
use crate::mm::pmm;
use onyx_core::errno::KResult;

pub struct EventRing {
    pub base: *mut Trb,
    pub phys: u64,
    pub size: u16,
    pub dequeue: u16,
    pub cycle: bool,
}

/// # Safety
///
/// `nentries` must be >= 1; PMM must be initialized; caller must publish
/// the result via ERST only after init.
pub unsafe fn alloc_event_ring(nentries: u16) -> KResult<EventRing> {
    // SAFETY: pa is a physically contiguous PMM allocation of
    // nentries*16 bytes, referenced by base/phys with size set to
    // nentries so all later indexing stays within it.
    unsafe {
        let n = nentries as usize;
        let bytes = n * 16;
        let pages = bytes.div_ceil(4096);
        let pa = pmm::alloc_n(pages)? as usize;
        Ok(EventRing {
            base: pa as *mut Trb,
            phys: pa as u64,
            size: nentries,
            dequeue: 0,
            // Consumer and producer both start at cycle 1 after HC reset.
            cycle: true,
        })
    }
}

/// # Safety
///
/// `er` must have been produced by `alloc_event_ring` (contiguous buffer
/// of `size` TRB entries); caller must hold exclusive access (IRQ-context
/// polling on the owning hart; no other hart may dequeue this ring).
pub unsafe fn poll_event(er: &mut EventRing) -> Option<Trb> {
    // SAFETY: dequeue is bounds-checked against size below and wraps at
    // size, so base.add(dequeue) stays within the allocated ring;
    // exclusive access held by caller.
    unsafe {
        let idx = er.dequeue as usize;
        if idx >= er.size as usize {
            return None;
        }
        // SAFETY: idx < er.size checked above, base covers size entries.
        let ev = &*er.base.add(idx);
        let c = (ev.params[3] & TRB_C) != 0;
        if c != er.cycle {
            return None;
        }
        let trb = *ev;
        er.dequeue += 1;
        if er.dequeue >= er.size {
            er.dequeue = 0;
            er.cycle = !er.cycle;
        }
        Some(trb)
    }
}

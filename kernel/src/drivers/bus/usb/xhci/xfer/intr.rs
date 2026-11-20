//! xHCI interrupt-IN transfers: the channel HID devices deliver reports on.
//!
//! Completions arrive on the SHARED event ring — xHCI has no per-device
//! used ring like virtio_input — so every transfer demuxes by endpoint
//! (`xfer::event_completes`, ruling 1), and a completion that lands while
//! a control transfer owns the ring is parked here for the poll. Arm
//! (`intr::arm`) programs the endpoint context and pre-posts N_BUFS
//! report buffers (ruling 2); the poll is NON-BLOCKING (ruling 2):
//! parked completions first, else one bounded ring drain — an empty
//! ring is idle — then copy the report and re-post the buffer that
//! carried it (also on an error completion, so polling may recover).

use core::ptr;
use onyx_core::errno::{Errno, KResult};

use super::super::{G_XHCI, ring};
use super::{completion_bytes, event_endpoint, try_transfer_event};

mod arm;
#[cfg(test)]
mod tests;

pub use arm::xhci_arm_interrupt;

/// Pre-posted buffer count (ruling 2): one ring slot per buffer, well
/// under the 31 usable TRB slots of the arm ring.
pub(super) const N_BUFS: usize = 8;
/// Interrupt transfer ring entries (same size as every driver ring);
/// shared by the arm (allocation) and the poll (TRB-pointer bounds).
pub(super) const RING_ENTRIES: u16 = 32;
/// Bytes per TRB — the stride used to bounds-check event TRB pointers.
const TRB_BYTES: u64 = 16;

/// DCI of an interrupt-IN endpoint address (§4.5.1): EP n IN → 2n+1.
/// `None` for EP0 or OUT endpoints — they never carry HID reports.
pub(super) fn ep_dci(ep_addr: u8) -> Option<u8> {
    let n = ep_addr & 0x0F;
    if ep_addr & 0x80 == 0 || n == 0 {
        return None;
    }
    Some(2 * n + 1)
}

/// One posted report buffer as a Normal TRB: buffer address split across
/// DW0/DW1, length in DW2, IOC in DW3 (each unchained buffer reports its
/// own completion); the cycle bit is left to `enqueue_trb`.
pub(super) fn intr_buf_trb(buf_pa: u64, len: u32) -> ring::Trb {
    let mut trb = ring::Trb::zero();
    trb.params[0] = buf_pa as u32;
    trb.params[1] = (buf_pa >> 32) as u32;
    trb.params[2] = len;
    trb.set_type(ring::TRB_NORMAL);
    trb.set_flags(ring::TRB_IOC);
    trb
}

/// Armed interrupt endpoint state: the driver arms one HID device (the
/// single live xHCI slot, `G_XHCI.slot`).
struct IntrState {
    armed: bool,
    /// Doorbell/context index of the armed interrupt-IN endpoint.
    dci: u8,
    slot_id: u8,
    max_pkt: u16,
    /// Interrupt ring base — bounds-check for completion TRB pointers.
    ring_pa: u64,
    /// Pre-posted buffer pool page (N_BUFS * max_pkt bytes).
    bufs_pa: u64,
}

// Single-owner state, like G_XHCI: written once at arm on the
// single-threaded boot probe, then read/updated on the serialized
// single-consumer poll path (SIE=0 per crate::sync).
static mut G_INTR: IntrState = IntrState {
    armed: false,
    dci: 0,
    slot_id: 0,
    max_pkt: 0,
    ring_pa: 0,
    bufs_pa: 0,
};

/// Completions parked while another endpoint's wait owned the event
/// ring; the poll pops them FIFO. Bounded by N_BUFS — one per buffer
/// that can be in flight.
static mut PARKED: [ring::Trb; N_BUFS] = [ring::Trb::zero(); N_BUFS];
static mut PARK_HEAD: usize = 0;
static mut PARK_LEN: usize = 0;

/// Park `ev` for the interrupt poll when it completes the armed
/// interrupt endpoint; returns whether it was parked. A full FIFO drops
/// the event (cannot happen: one park per in-flight buffer) — its
/// buffer then simply never gets re-posted.
///
/// # Safety
///
/// Single-owner event-ring path only (SIE=0, all USB transfers
/// serialized): mutates the parked-event statics.
pub(super) unsafe fn park_if_armed(ev: &ring::Trb) -> bool {
    // SAFETY: single-owner path per the function contract.
    unsafe {
        let g = &raw const G_INTR;
        if !(*g).armed || event_endpoint(ev) != u32::from((*g).dci) {
            return false;
        }
        if PARK_LEN >= N_BUFS {
            return false;
        }
        // SAFETY: single-owner path; the index stays below N_BUFS.
        PARKED[(PARK_HEAD + PARK_LEN) % N_BUFS] = *ev;
        PARK_LEN += 1;
        true
    }
}

/// Pop the oldest parked completion, if any.
///
/// # Safety
///
/// Single-owner event-ring path only.
unsafe fn take_parked() -> Option<ring::Trb> {
    // SAFETY: single-owner path per the function contract.
    unsafe {
        if PARK_LEN == 0 {
            return None;
        }
        let ev = PARKED[PARK_HEAD];
        PARK_HEAD = (PARK_HEAD + 1) % N_BUFS;
        PARK_LEN -= 1;
        Some(ev)
    }
}

/// Poll one HID report off the armed interrupt-IN endpoint into `buf`
/// (which must hold at least the endpoint's max packet size) —
/// NON-BLOCKING (ruling 2): serve a parked completion first, else ONE
/// bounded drain of the shared event ring filtered to the armed DCI;
/// a ring without a completion for the DCI is IDLE, not a wait. On a
/// completion, recover the buffer that carried it, re-post it so the
/// pool stays N_BUFS deep, and copy the report out.
///
/// Contract (mirroring the OHCI poll):
/// - `Ok(0)` — armed, nothing pending. Returns immediately; the
///   100 Hz input-poll loop may spin on this cheaply.
/// - `Ok(n > 0)` — one report of n bytes copied into `buf`.
/// - `Err(Errno::Io)` — the completion retired with an error code
///   (stall, no-response, ...). The carried buffer is re-posted
///   BEFORE the error surfaces, so polling may recover — the OHCI
///   contract's stance (its poll re-arms the TD on error).
/// - `Err(Errno::NoEnt)` — the channel was never armed;
///   `Err(Errno::Inval)` — `buf` is smaller than the endpoint's max
///   packet.
///
/// # Safety
///
/// Single-consumer poll context only (SIE=0 per `crate::sync`,
/// serialized with every other xHCI transfer path); the endpoint must
/// have been armed by [`xhci_arm_interrupt`]. The registry lock is NOT
/// taken — the caller must not hold USB_DEV_LOCK, or the dispatcher's
/// registry read would deadlock.
pub(crate) unsafe fn xhci_poll_interrupt(buf: &mut [u8]) -> KResult<u32> {
    // SAFETY: static-driver state on the serialized single-owner path.
    unsafe {
        let ctx = &raw mut G_XHCI;
        if !(*ctx).operational {
            return Err(Errno::NoSys);
        }
        let g = &raw const G_INTR;
        if !(*g).armed {
            return Err(Errno::NoEnt);
        }
        if buf.len() < (*g).max_pkt as usize {
            return Err(Errno::Inval);
        }
        let dci = (*g).dci;
        // Fast path (ruling 2): parked completions first, else one
        // bounded ring drain. No completion for the armed DCI → idle.
        let ev = match take_parked() {
            Some(ev) => ev,
            None => match try_transfer_event(dci) {
                Some(ev) => ev,
                None => return Ok(0),
            },
        };
        // The completion names the posted TRB in DW0/DW1 (§6.4.2.1);
        // read it back to recover the buffer it carried.
        let trb_pa = (ev.params[0] as u64) | ((ev.params[1] as u64) << 32);
        let ring_end = (*g).ring_pa + u64::from(RING_ENTRIES) * TRB_BYTES;
        if trb_pa < (*g).ring_pa || trb_pa >= ring_end {
            return Err(Errno::Io);
        }
        // SAFETY: trb_pa is inside the interrupt ring's PMM allocation
        // (identity-mapped, bounds-checked above) and still holds the
        // posted TRB — 31 ring slots against N_BUFS in flight.
        let posted = &*(trb_pa as *const ring::Trb);
        let buf_pa = (posted.params[0] as u64) | ((posted.params[1] as u64) << 32);
        if buf_pa < (*g).bufs_pa
            || buf_pa >= (*g).bufs_pa + (N_BUFS as u64) * u64::from((*g).max_pkt)
        {
            return Err(Errno::Io);
        }
        // Re-post the buffer BEFORE classifying the completion: an
        // error completion retires its TRB the same way a success
        // does, and a N_BUFS-deep pool is what lets polling recover.
        let ring_ptr = (*ctx).xfer_rings[dci as usize - 1];
        if ring_ptr.is_null() {
            return Err(Errno::Io); // armed implies published; defensive
        }
        let trb = intr_buf_trb(buf_pa, u32::from((*g).max_pkt));
        // SAFETY: ring_ptr holds the armed interrupt TrbRing.
        ring::enqueue_trb(&mut *ring_ptr, &trb);
        ring::ring_doorbell((*g).slot_id, dci);
        match completion_bytes(&ev, u32::from((*g).max_pkt)) {
            // Ok(0): a zero-length completion carries no report —
            // idle semantics, the buffer is already back in the pool.
            Ok(n) => {
                // SAFETY: buf_pa is inside the pre-posted buffer page
                // (checked above); n ≤ max_pkt ≤ buf.len().
                ptr::copy_nonoverlapping(buf_pa as *const u8, buf.as_mut_ptr(), n as usize);
                Ok(n)
            }
            Err(e) => Err(e),
        }
    }
}

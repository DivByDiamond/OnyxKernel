//! xHCI interrupt-IN transfers: the channel HID devices deliver reports on.
//!
//! Completions arrive on the SHARED event ring — xHCI has no per-device
//! used ring like virtio_input — so every transfer demuxes by (slot,
//! endpoint) (`xfer::event_completes`), and a completion that lands
//! while another transfer owns the ring is parked in its OWNING
//! device's FIFO (`devs::park_if_armed`) for that device's poll. Arm
//! (`intr::arm`) programs the endpoint context and pre-posts N_BUFS
//! report buffers (ruling 2); the poll is NON-BLOCKING (ruling 2):
//! parked completions first, else one bounded ring drain — an empty
//! ring is idle — then copy the report and re-post the buffer that
//! carried it (also on an error completion, so polling may recover).

use core::ptr;
use onyx_core::errno::{Errno, KResult};

use super::super::devs::{self, N_BUFS};
use super::super::ring;
use super::{completion_bytes, try_transfer_event};

mod arm;
#[cfg(test)]
mod tests;

pub use arm::xhci_arm_interrupt;

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

/// Is `addr` inside `[base, base + span)`? Overflow-safe: hostile
/// event pointers must not wrap the bound.
pub(super) fn in_range(base: u64, span: u64, addr: u64) -> bool {
    addr >= base && addr - base < span
}

/// Poll one HID report off the interrupt-IN endpoint armed on
/// controller slot `slot` into `buf` (which must hold at least the
/// endpoint's max packet size) — NON-BLOCKING (ruling 2): serve a
/// parked completion first, else ONE bounded drain of the shared event
/// ring filtered to the slot's (slot, DCI); a ring without a completion
/// for it is IDLE, not a wait. Completions of OTHER devices met during
/// the drain are parked on their owners, never consumed. On a
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
/// - `Err(Errno::NoEnt)` — no such device or it was never armed;
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
pub(crate) unsafe fn xhci_poll_interrupt(slot: u8, buf: &mut [u8]) -> KResult<u32> {
    // SAFETY: static-driver state on the serialized single-owner path.
    // Table references are scoped so none lives across the event-ring
    // drain, which re-enters the table to park other devices' events.
    unsafe {
        if !super::super::is_operational() {
            return Err(Errno::NoSys);
        }
        let (dci, max_pkt, ring_pa, bufs_pa, parked) = {
            let d = devs::dev(slot).ok_or(Errno::NoEnt)?;
            if !d.intr.armed {
                return Err(Errno::NoEnt);
            }
            let c = &mut d.intr;
            (c.dci, c.max_pkt, c.ring_pa, c.bufs_pa, c.take_parked())
        };
        if buf.len() < max_pkt as usize {
            return Err(Errno::Inval);
        }
        // Fast path (ruling 2): parked completions first, else one
        // bounded ring drain. No completion for the armed DCI → idle.
        let ev = match parked {
            Some(ev) => ev,
            None => match try_transfer_event(slot, dci) {
                Some(ev) => ev,
                None => return Ok(0),
            },
        };
        // The completion names the posted TRB in DW0/DW1 (§6.4.2.1);
        // read it back to recover the buffer it carried.
        let trb_pa = (ev.params[0] as u64) | ((ev.params[1] as u64) << 32);
        if !in_range(ring_pa, u64::from(RING_ENTRIES) * TRB_BYTES, trb_pa) {
            return Err(Errno::Io);
        }
        // SAFETY: trb_pa is inside the interrupt ring's PMM allocation
        // (identity-mapped, bounds-checked above) and still holds the
        // posted TRB — 31 ring slots against N_BUFS in flight.
        let posted = &*(trb_pa as *const ring::Trb);
        let buf_pa = (posted.params[0] as u64) | ((posted.params[1] as u64) << 32);
        if !in_range(bufs_pa, (N_BUFS as u64) * u64::from(max_pkt), buf_pa) {
            return Err(Errno::Io);
        }
        // Re-post the buffer BEFORE classifying the completion: an
        // error completion retires its TRB the same way a success
        // does, and a N_BUFS-deep pool is what lets polling recover.
        let trb = intr_buf_trb(buf_pa, u32::from(max_pkt));
        let ring = devs::dev(slot)
            .and_then(|d| d.ring_mut(dci))
            .ok_or(Errno::Io)?; // armed implies published; defensive
        ring::enqueue_trb(ring, &trb);
        ring::ring_doorbell(slot, dci);
        match completion_bytes(&ev, u32::from(max_pkt)) {
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

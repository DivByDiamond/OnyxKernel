//! Event-ring waits for xHCI transfers: the shared event ring is demuxed
//! by (slot, endpoint) and by TRB pointer, and completions that belong
//! to another device's armed interrupt endpoint are parked on that
//! device instead of being dropped.

use onyx_core::errno::{Errno, KResult};

use super::super::devs::{self, event_dci, event_slot};
use super::super::{G_XHCI, XhciCtx, regs, ring};

/// Poll budget for one transfer to finish (busy-wait, mirroring the
/// EHCI transfer loops).
const XFER_TIMEOUT: u32 = 500_000;
/// ERDP bit 3: Event Handler Busy — RW1C, written to clear after dequeue.
const ERDP_EHB: u64 = 1 << 3;

/// Does `ev` complete a transfer on endpoint `dci` of controller slot
/// `slot`? Command completions, port status changes and other
/// endpoints' — or other DEVICES' — completions satisfy no single-
/// endpoint wait: the shared event ring is demuxed by (slot, endpoint).
pub(in crate::drivers::bus::usb::xhci) fn event_completes(
    ev: &ring::Trb,
    slot: u8,
    dci: u8,
) -> bool {
    let ty = (ev.params[3] >> 10) & 0x3F;
    ty == ring::TRB_TRANSFER_EVENT && event_slot(ev) == slot && event_dci(ev) == dci
}

/// TRB Pointer of a transfer event (§6.4.2.1): the 64-bit DW0/DW1
/// address of the TRB that generated it.
fn event_trb_ptr(ev: &ring::Trb) -> u64 {
    ev.params[0] as u64 | ((ev.params[1] as u64) << 32)
}

/// Does `ev` report one of `want` — the TRBs the caller just enqueued
/// for its own transfer? Matching on the TRB pointer (not the endpoint
/// id alone) is what keeps a wait honest: a short control read leaves
/// a SECOND event (the status stage's IOC) in the ring, and without
/// this filter the NEXT transfer's wait would match that stale event
/// and report its own length as fully transferred.
fn reports_wanted(ev: &ring::Trb, want: &[u64]) -> bool {
    want.contains(&event_trb_ptr(ev))
}

/// Physical address of the TRB the caller's i-th upcoming enqueue will
/// write — the same wrap math `ring::enqueue_trb` applies (the link
/// TRB at index size-1 is never written, the cursor wraps there).
pub(in crate::drivers::bus::usb::xhci) fn trb_addr_at(ring: &ring::TrbRing, i: usize) -> u64 {
    let usable = (ring.size as usize).saturating_sub(1);
    if usable == 0 {
        return ring.phys;
    }
    ring.phys + 16 * ((ring.enqueue as usize + i) % usable) as u64
}

/// # Safety
///
/// `ctx` must be the driver's `G_XHCI` with `rtsoff`/`event_ring` set by
/// `init`; single-owner event-ring consumer (no concurrent dequeue).
pub(in crate::drivers::bus::usb::xhci) unsafe fn ack_event_ring(ctx: *mut XhciCtx) {
    // SAFETY: ctx is static G_XHCI on the single-owner path; event_ring
    // was allocated by init, so phys + dequeue*16 addresses a valid TRB.
    unsafe {
        let deq = (*ctx).event_ring.phys + (*ctx).event_ring.dequeue as u64 * 16;
        regs::rt_w64((*ctx).rtsoff, 0, regs::RTS_ERDP, deq | ERDP_EHB);
    }
}

/// # Safety
///
/// Requires `init` to have run; the caller owns the controller (no
/// concurrent command or transfer is in flight) and every xHCI transfer
/// path is serialized (single-consumer, SIE=0 per `crate::sync`). The
/// wait consumes and drops events that complete no transfer on `dci`
/// — except completions of an ARMED interrupt endpoint, which are
/// parked for its poll instead (`devs::park_if_armed`). `want` holds
/// the physical TRB addresses of the caller's own in-flight TD; only a
/// transfer event pointing inside it completes the wait.
pub(in crate::drivers::bus::usb::xhci) unsafe fn wait_transfer_event(
    slot: u8,
    dci: u8,
    want: &[u64],
) -> KResult<ring::Trb> {
    // SAFETY: ctx is static G_XHCI (init ran); poll_event bounds-checks its
    // index, and ack_event_ring re-borrows only after the poll borrow ends.
    unsafe {
        let ctx = &raw mut G_XHCI;
        let mut timeout = XFER_TIMEOUT;
        while timeout > 0 {
            let ev = {
                let er = &mut (*ctx).event_ring;
                ring::poll_event(er)
            };
            if let Some(ev) = ev {
                ack_event_ring(ctx);
                if event_completes(&ev, slot, dci) && reports_wanted(&ev, want) {
                    return Ok(ev);
                }
                // Not this endpoint's completion: command completions and
                // port changes are consumed and dropped (the same stance
                // as submit_command's handling of transfer events; nothing
                // else on this serialized path can be waiting on them).
                devs::park_if_armed(&ev);
            }
            timeout -= 1;
        }
        Err(Errno::Io)
    }
}

/// Non-blocking event-ring probe for the interrupt poll (ruling 2):
/// dequeue whatever is pending — the drain is bounded by the ring's
/// own size — and return the first completion of endpoint `dci`;
/// events that complete no `dci` transfer are consumed and dropped,
/// the same stance as [`wait_transfer_event`]. `None` means the ring
/// ran dry: the armed endpoint is IDLE, report nothing at once — the
/// 100 Hz input-poll loop spins on this cheaply.
///
/// # Safety
///
/// Requires `init` to have run; the caller owns the controller (no
/// concurrent command or transfer is in flight) and every xHCI
/// transfer path is serialized (single-consumer, SIE=0 per
/// `crate::sync`).
pub(in crate::drivers::bus::usb::xhci) unsafe fn try_transfer_event(
    slot: u8,
    dci: u8,
) -> Option<ring::Trb> {
    // SAFETY: ctx is static G_XHCI (init ran); poll_event
    // bounds-checks its index, and ack_event_ring re-borrows only
    // after the poll borrow ends.
    unsafe {
        let ctx = &raw mut G_XHCI;
        loop {
            let ev = {
                let er = &mut (*ctx).event_ring;
                ring::poll_event(er)
            }?;
            ack_event_ring(ctx);
            if event_completes(&ev, slot, dci) {
                return Some(ev);
            }
            // Not this endpoint's completion: command completions and
            // port changes are consumed and dropped. Armed-interrupt
            // completions still go to the park FIFO — a caller
            // waiting on another endpoint (EP0) must not eat them.
            devs::park_if_armed(&ev);
        }
    }
}

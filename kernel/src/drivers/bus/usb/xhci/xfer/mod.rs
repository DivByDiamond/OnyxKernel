//! xHCI control transfers: Setup/Data/Status stage TRBs on the EP0 ring.
//!
//! Mirrors `ehci::xfer`: build the TD, ring the doorbell, busy-poll the
//! event ring for the completion, map the residual length to bytes done.
pub(super) mod bulk;

use onyx_core::errno::{Errno, KResult};

use super::{G_XHCI, XhciCtx, regs, ring};

/// Poll budget for one transfer to finish (busy-wait, mirroring the
/// EHCI transfer loops).
const XFER_TIMEOUT: u32 = 500_000;
/// Completion codes that complete a transfer: Success, and Short Packet —
/// a short IN read (GET_DESCRIPTOR) is normal, not an error.
const CC_SUCCESS: u32 = 1;
const CC_SHORT_PACKET: u32 = 13;
/// ERDP bit 3: Event Handler Busy — RW1C, written to clear after dequeue.
const ERDP_EHB: u64 = 1 << 3;

/// # Safety
///
/// `ctx` must be the driver's `G_XHCI` with `rtsoff`/`event_ring` set by
/// `init`; single-owner event-ring consumer (no concurrent dequeue).
unsafe fn ack_event_ring(ctx: *mut XhciCtx) {
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
/// concurrent command or transfer is in flight), so any transfer event in
/// the ring belongs to the caller's just-submitted TD.
pub(super) unsafe fn wait_transfer_event() -> KResult<ring::Trb> {
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
                let ty = (ev.params[3] >> 10) & 0x3F;
                if ty == ring::TRB_TRANSFER_EVENT {
                    return Ok(ev);
                }
                // Port-change / stray events are consumed and dropped:
                // this driver has no hotplug consumer (same stance as
                // submit_command's handling of transfer events).
            }
            timeout -= 1;
        }
        Err(Errno::Io)
    }
}

/// Map a transfer event to bytes actually transferred: the event carries
/// the residual (bytes NOT moved) and the completion code in DW2.
fn completion_bytes(ev: &ring::Trb, data_len: u32) -> KResult<u32> {
    let cc = (ev.params[2] >> 24) & 0xFF;
    if cc != CC_SUCCESS && cc != CC_SHORT_PACKET {
        return Err(Errno::Io);
    }
    Ok(data_len.saturating_sub(ev.params[2] & 0x00FF_FFFF))
}

/// Build the Setup/Data/Status TRB sequence for one control TD.
/// Returns the TRBs in order and how many are live (2 or 3).
///
/// # Safety
///
/// Pure field arithmetic on caller-owned `Trb`s (required unsafe only by
/// the project-wide xHCI convention, matching `ring::setup_trb`).
pub(super) unsafe fn control_trbs(
    setup_pkt: &[u8; 8],
    data_buf: u64,
    data_len: usize,
    data_in: bool,
) -> ([ring::Trb; 3], usize) {
    // SAFETY: no memory effects — writes land in the returned stack array.
    unsafe {
        let mut trbs = [ring::Trb::zero(); 3];
        ring::setup_trb(
            &mut trbs[0],
            setup_pkt[1],
            setup_pkt[0],
            u16::from_be_bytes([setup_pkt[2], setup_pkt[3]]),
            u16::from_be_bytes([setup_pkt[4], setup_pkt[5]]),
            u16::from_be_bytes([setup_pkt[6], setup_pkt[7]]),
        );
        trbs[0].set_flags(ring::TRB_CH); // a Status Stage always follows
        let mut n = 1;
        if data_len > 0 {
            trbs[1].params[0] = data_buf as u32;
            trbs[1].params[1] = (data_buf >> 32) as u32;
            trbs[1].params[2] = data_len as u32;
            trbs[1].set_type(ring::TRB_DATA);
            trbs[1].set_flags(ring::TRB_CH | if data_in { 1 << 16 } else { 0 });
            n = 2;
        }
        // Status Stage: opposite the data stage (IN when there is none);
        // IOC is where this TD's transfer event comes from.
        trbs[n].set_type(ring::TRB_STATUS);
        trbs[n].set_flags(
            ring::TRB_IOC
                | if data_len == 0 || !data_in {
                    1 << 16
                } else {
                    0
                },
        );
        (trbs, n + 1)
    }
}

/// # Safety
///
/// Requires `init` to have run (controller operational, EP0 ring in
/// `xfer_rings[0]`, a device addressed into `G_XHCI.slot`) and the caller
/// to serialize USB access (single-threaded path, SIE=0 per `crate::sync`).
/// `setup_pkt` and `data` must remain valid and DMA-addressable for the
/// duration of the transfer; `data` must cover the setup packet's wLength.
///
/// `dev_addr`/`max_pkt` are unused: xHCI addresses transfers by slot id
/// and keeps the packet size in the EP0 context (kept for URB parity with
/// the EHCI/OHCI signatures).
pub(crate) unsafe fn xhci_control_transfer(
    _dev_addr: u8,
    setup_pkt: &[u8; 8],
    mut data: Option<&mut [u8]>,
    data_in: bool,
    _max_pkt: u32,
) -> KResult<u32> {
    // SAFETY: init allocated the EP0 ring and set operational; raw-ptr
    // accesses to G_XHCI are transient on the single-owner path; the data
    // buffer is caller-provided and valid for the transfer.
    unsafe {
        let ctx = &raw mut G_XHCI;
        if !(*ctx).operational {
            return Err(Errno::NoSys);
        }
        let slot = (*ctx).slot;
        let ring_ptr = (*ctx).xfer_rings[0];
        if slot == 0 || ring_ptr.is_null() {
            return Err(Errno::NoEnt);
        }
        let data_len = data.as_ref().map_or(0, |d| d.len());
        if data_len > 0x1_0000 {
            return Err(Errno::Inval); // Data Stage TRB caps at 64 KiB
        }
        let data_buf = match data.as_mut() {
            Some(d) => d.as_mut_ptr() as u64,
            None => 0,
        };
        let (trbs, n) = control_trbs(setup_pkt, data_buf, data_len, data_in);
        let ring = &mut *ring_ptr;
        for trb in trbs.iter().take(n) {
            ring::enqueue_trb(ring, trb);
        }
        ring::ring_doorbell(slot, 1); // DCI 1 = EP0
        let ev = wait_transfer_event()?;
        completion_bytes(&ev, data_len as u32)
    }
}

#[cfg(test)]
mod tests;

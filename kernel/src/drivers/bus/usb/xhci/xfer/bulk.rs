//! xHCI bulk transfers: one Normal TRB on the endpoint's transfer ring.
use onyx_core::errno::{Errno, KResult};

use super::super::devs;
use super::super::ring;
use super::{completion_bytes, trb_addr_at, wait_transfer_event};

/// Driver endpoint-context index for a USB endpoint address: 0 = EP0, then
/// EP1-out, EP1-in, EP2-out, ... — xHCI endpoint contexts are
/// direction-specific, so the address's direction bit picks the pair.
pub(super) fn ep_ctx_index(endpoint: u8) -> u8 {
    let n = endpoint & 0x0F;
    if n == 0 {
        return 0;
    }
    (n - 1) * 2 + 1 + u8::from(endpoint & 0x80 != 0)
}

/// # Safety
///
/// Requires `init` to have run (controller operational) and the device
/// of controller slot `slot` addressed, with this endpoint configured
/// by the bring-up caller (its transfer ring published in the
/// per-device table). The caller serializes USB access
/// (single-threaded path, SIE=0 per `crate::sync`), and `data` stays
/// valid and DMA-addressable for the duration of the transfer.
///
/// The first parameter is the controller slot id: xHCI addresses
/// transfers by slot (kept first for URB parity with the EHCI/OHCI
/// signatures). Direction comes from the endpoint address — the
/// endpoint context carries it, not the TRB.
pub(crate) unsafe fn xhci_bulk_transfer(
    slot: u8,
    endpoint: u8,
    mut data: Option<&mut [u8]>,
) -> KResult<u32> {
    // SAFETY: init allocated the controller rings; the per-device
    // table is single-owner here, with no reference held across the
    // event wait; the data buffer is caller-provided and valid.
    unsafe {
        if !super::super::is_operational() {
            return Err(Errno::NoSys);
        }
        let idx = ep_ctx_index(endpoint) as usize;
        if idx == 0 || idx >= 32 {
            return Err(Errno::Inval); // bulk never runs on EP0
        }
        let dci = (idx + 1) as u8; // DCI = context index
        let data_len = data.as_ref().map_or(0, |d| d.len());
        if data_len == 0 {
            return Ok(0);
        }
        if data_len > 0x1_0000 {
            return Err(Errno::NoMem); // one Normal TRB, 64 KiB cap
        }
        let data_buf = match data.as_mut() {
            Some(d) => d.as_mut_ptr() as u64,
            None => return Ok(0),
        };
        let mut trb = ring::Trb::zero();
        trb.params[0] = data_buf as u32;
        trb.params[1] = (data_buf >> 32) as u32;
        trb.params[2] = data_len as u32;
        trb.set_type(ring::TRB_NORMAL);
        trb.set_flags(ring::TRB_IOC);
        let want = {
            let ring = devs::dev(slot)
                .ok_or(Errno::NoEnt)?
                .ring_mut(dci)
                .ok_or(Errno::NoEnt)?; // endpoint not configured yet
            // One Normal TRB: its address is the wait's wanted pointer.
            let want = [trb_addr_at(ring, 0)];
            ring::enqueue_trb(ring, &trb);
            want
        };
        ring::ring_doorbell(slot, dci);
        let ev = wait_transfer_event(slot, dci, &want)?;
        completion_bytes(&ev, data_len as u32)
    }
}

#[cfg(test)]
mod tests;

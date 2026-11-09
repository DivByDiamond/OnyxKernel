//! xHCI bulk transfers: one Normal TRB on the endpoint's transfer ring.
use onyx_core::errno::{Errno, KResult};

use super::super::{G_XHCI, ring};
use super::{completion_bytes, wait_transfer_event};

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
/// Requires `init` to have run (controller operational) and this endpoint
/// configured via `device::configure_endpoint` (its transfer ring in
/// `xfer_rings[idx]`); `G_XHCI.slot` must hold a granted, addressed slot.
/// The caller serializes USB access (single-threaded path, SIE=0 per
/// `crate::sync`), and `data` stays valid and DMA-addressable for the
/// duration of the transfer.
///
/// `dev_addr` is unused: xHCI addresses transfers by slot id (kept for
/// URB parity with the EHCI/OHCI signatures). Direction comes from the
/// endpoint address — the endpoint context carries it, not the TRB.
pub(crate) unsafe fn xhci_bulk_transfer(
    _dev_addr: u8,
    endpoint: u8,
    mut data: Option<&mut [u8]>,
) -> KResult<u32> {
    // SAFETY: init allocated the controller rings; raw-ptr accesses to
    // G_XHCI are transient on the single-owner path; the data buffer is
    // caller-provided and valid for the transfer.
    unsafe {
        let ctx = &raw mut G_XHCI;
        if !(*ctx).operational {
            return Err(Errno::NoSys);
        }
        let slot = (*ctx).slot;
        if slot == 0 {
            return Err(Errno::NoEnt);
        }
        let idx = ep_ctx_index(endpoint) as usize;
        if idx == 0 || idx >= 32 {
            return Err(Errno::Inval); // bulk never runs on EP0
        }
        let ring_ptr = (*ctx).xfer_rings[idx];
        if ring_ptr.is_null() {
            return Err(Errno::NoEnt); // endpoint not configured yet
        }
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
        let ring = &mut *ring_ptr;
        ring::enqueue_trb(ring, &trb);
        ring::ring_doorbell(slot, (idx + 1) as u8); // DCI = context index
        let ev = wait_transfer_event()?;
        completion_bytes(&ev, data_len as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::ep_ctx_index;

    #[test]
    fn endpoint_addresses_map_to_direction_specific_contexts() {
        assert_eq!(ep_ctx_index(0x00), 0); // EP0
        assert_eq!(ep_ctx_index(0x01), 1); // EP1 OUT
        assert_eq!(ep_ctx_index(0x81), 2); // EP1 IN
        assert_eq!(ep_ctx_index(0x02), 3); // EP2 OUT
        assert_eq!(ep_ctx_index(0x82), 4); // EP2 IN
        assert_eq!(ep_ctx_index(0x0F), 29); // EP15 OUT
        assert_eq!(ep_ctx_index(0x8F), 30); // EP15 IN
    }
}

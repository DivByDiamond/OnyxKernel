//! xHCI control transfers: Setup/Data/Status stage TRBs on the EP0 ring.
//!
//! Mirrors `ehci::xfer`: build the TD, ring the doorbell, busy-poll the
//! event ring for the completion, map the residual length to bytes done.
pub(super) mod bulk;
pub(super) mod intr;
mod wait;

pub(super) use wait::{ack_event_ring, trb_addr_at, try_transfer_event, wait_transfer_event};

use onyx_core::errno::{Errno, KResult};

use super::devs;
use super::ring;

/// DCI of the EP0 transfer ring — the endpoint every control transfer
/// waits on (§4.5.1).
pub(super) const DCI_EP0: u8 = 1;

/// Completion codes that complete a transfer: Success, and Short Packet —
/// a short IN read (GET_DESCRIPTOR) is normal, not an error.
const CC_SUCCESS: u32 = 1;
const CC_SHORT_PACKET: u32 = 13;

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
/// `setup_pkt` carries the packet's WIRE bytes — the same little-endian
/// 16-bit fields `requests::setup` emits (bmRequestType, bRequest, then
/// wValue/wIndex/wLength LSB-first), USB 2.0 §9.4.3 (wValue packs the
/// descriptor type in its high byte for GET_DESCRIPTOR).
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
            u16::from_le_bytes([setup_pkt[2], setup_pkt[3]]),
            u16::from_le_bytes([setup_pkt[4], setup_pkt[5]]),
            u16::from_le_bytes([setup_pkt[6], setup_pkt[7]]),
        );
        trbs[0].set_flags(ring::TRB_CH); // a Status Stage always follows
        let mut n = 1;
        if data_len > 0 {
            trbs[1].params[0] = data_buf as u32;
            trbs[1].params[1] = (data_buf >> 32) as u32;
            trbs[1].params[2] = data_len as u32;
            trbs[1].set_type(ring::TRB_DATA);
            // ISP (§6.4.1.2.1): a short data stage reports at the DATA
            // TRB with the true residual; without it the only transfer
            // event comes from the STATUS TRB (IOC), whose residual is
            // always 0 — every read would "complete" with the full
            // requested length (a 512-byte GET_DESCRIPTOR report read
            // would parse 460 bytes of garbage after the descriptor).
            trbs[1].set_flags(ring::TRB_CH | ring::TRB_ISP);
            if data_in {
                trbs[1].set_flags(1 << 16); // DIR: IN (DW3 bit 16)
            }
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
/// Requires `init` to have run (controller operational) and the device
/// of controller slot `slot` addressed by `bringup_device` (its EP0
/// ring published in the per-device table), and the caller to
/// serialize USB access (single-threaded path, SIE=0 per
/// `crate::sync`). `setup_pkt` and `data` must remain valid and
/// DMA-addressable for the duration of the transfer; `data` must cover
/// the setup packet's wLength.
///
/// The first parameter is the controller SLOT ID (what `LiveBus`
/// carries for an xHCI device) — xHCI addresses transfers by slot, not
/// by USB address. An unknown or never-addressed slot is `NoEnt`.
/// `max_pkt` is unused: the packet size lives in the EP0 context (kept
/// for URB parity with the EHCI/OHCI signatures).
pub(crate) unsafe fn xhci_control_transfer(
    slot: u8,
    setup_pkt: &[u8; 8],
    mut data: Option<&mut [u8]>,
    data_in: bool,
    _max_pkt: u32,
) -> KResult<u32> {
    // SAFETY: init set operational; the per-device table is
    // single-owner here; the data buffer is caller-provided and valid
    // for the transfer. No table reference lives across the event wait
    // (it re-enters the table to park other devices' completions).
    unsafe {
        if !super::is_operational() {
            return Err(Errno::NoSys);
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
        // The TRB addresses this TD will occupy: transfer events match
        // by TRB pointer (see wait_transfer_event).
        let mut want = [0u64; 3];
        {
            let ring = devs::dev(slot)
                .and_then(|d| d.ring_mut(DCI_EP0))
                .ok_or(Errno::NoEnt)?;
            for (i, w) in want.iter_mut().enumerate().take(n) {
                *w = trb_addr_at(ring, i);
            }
            for trb in trbs.iter().take(n) {
                ring::enqueue_trb(ring, trb);
            }
        }
        ring::ring_doorbell(slot, DCI_EP0);
        let ev = wait_transfer_event(slot, DCI_EP0, &want[..n])?;
        completion_bytes(&ev, data_len as u32)
    }
}

#[cfg(test)]
mod tests;

//! The arm half of the interrupt-IN channel: registry lookup, endpoint
//! context programming (Configure Endpoint) and buffer pre-posting.

use core::ptr;
use onyx_core::errno::{Errno, KResult};
use onyx_core::fmt::Arg;

use super::super::super::device::{EP_TYPE_INT_IN, SPEED_HS, SPEED_SS, ctx};
use super::super::super::{G_XHCI, ring};
use super::super::CC_SUCCESS;
use super::{IntrState, N_BUFS, RING_ENTRIES, ep_dci, intr_buf_trb};
use crate::drivers::usb::dev::{Owner, slot as registry_slot};
use crate::mm::pmm;

/// Legal interrupt-IN max packet range (ruling 2: 8-64 byte buffers).
const MIN_MPS: u16 = 8;
const MAX_MPS: u16 = 64;

/// Endpoint-context Interval field for an interrupt endpoint of a device
/// running at `speed` with descriptor `b_interval` (§6.2.3.6 Table 6-12).
/// HS/SS descriptors carry the period exponent (125 µs units): field =
/// bInterval-1. FS/LS descriptors carry milliseconds: field =
/// floor(log2(bInterval))+3, i.e. the period rounds DOWN to a power of
/// two (10 ms → 8 ms) — the host then polls slightly FASTER than the
/// device asked, which is the safe direction: a slower poll could
/// violate the device's latency requirement. QEMU tolerance of this
/// encoding is unvalidated until the Task 11 smoke.
pub(super) fn encode_interval(speed: u8, b_interval: u8) -> u8 {
    match speed {
        SPEED_HS | SPEED_SS => b_interval.clamp(1, 16) - 1,
        _ => {
            let frames = u32::from(b_interval.clamp(1, 255));
            let log2 = u32::BITS - 1 - frames.leading_zeros();
            (log2 + 3).clamp(3, 10) as u8
        }
    }
}

/// The five DWs of an interrupt-IN endpoint context (§6.2.3 Tables
/// 6-8..6-11): Interval in DW0, MaxPkt/EPType/CErr in DW1, the dequeue
/// pointer with DCS in DW2/DW3, Average TRB Length and Max ESIT Payload
/// (max_pkt for a burst-0 endpoint) in DW4.
pub(super) fn intr_ep_words(interval: u8, max_pkt: u16, deq: u64, dcs: bool) -> [u32; 5] {
    [
        u32::from(interval) << 16,
        (max_pkt as u32) << 16 | (EP_TYPE_INT_IN as u32) << 3 | ctx::CERR << 1,
        (deq as u32 & !0xF) | u32::from(dcs),
        (deq >> 32) as u32,
        u32::from(max_pkt) | (u32::from(max_pkt) << 16),
    ]
}

/// Arm the interrupt-IN endpoint of registry slot `slot_idx`: program
/// the endpoint context (DCI = 2*ep+1, EP Type 7; Configure Endpoint
/// command, TRB type 12 — verified against QEMU's hcd-xhci command
/// table the same way task 3 verified the speed bits), create the
/// interrupt ring and pre-post N_BUFS buffers with one doorbell.
///
/// The interval encoding is documented on [`encode_interval`] and is
/// unvalidated against QEMU until the Task 11 smoke.
///
/// # Safety
///
/// Requires a prior successful `xhci::init` and `bringup_device` (the
/// slot live, its device context published in the DCBAA); boot-probe
/// context only — single-threaded, SIE=0, USB_DEV_LOCK not held (the
/// registry is read under its own lock inside).
pub unsafe fn xhci_arm_interrupt(slot_idx: usize) -> KResult<()> {
    let s = registry_slot(slot_idx).ok_or(Errno::NoEnt)?;
    if s.owner != Owner::Xhci {
        return Err(Errno::NoSys); // the dispatcher never routes these here
    }
    let dci = ep_dci(s.ep_in).ok_or(Errno::Inval)?;
    if !(MIN_MPS..=MAX_MPS).contains(&s.max_packet) {
        return Err(Errno::Inval);
    }
    unsafe {
        let g = &raw mut G_XHCI;
        if !(*g).operational {
            return Err(Errno::NoSys);
        }
        let slot_id = (*g).slot;
        if slot_id == 0 {
            return Err(Errno::NoEnt);
        }
        // Interrupt transfer ring, published exactly like EP0's.
        let xfer_ring = ring::alloc_ring(RING_ENTRIES)?;
        let ring_pa = xfer_ring.phys;
        let dcs = xfer_ring.cycle;
        // SAFETY: fresh zeroed PMM allocation; ptr::write fully
        // initializes the TrbRing before it is published below.
        let ring_ptr = pmm::alloc_zero()? as *mut ring::TrbRing;
        ptr::write(ring_ptr, xfer_ring);
        (*g).xfer_rings[dci as usize - 1] = ring_ptr;

        // Input context on one zeroed page: ICC + a slot-context copy
        // (A0 required — QEMU rejects add flags without the slot bit —
        // with Context Entries raised to the new DCI) + the EP context.
        // SAFETY: one zeroed PMM page; the writes below stay within
        // offsets 0..(64 + dci*32) ≤ 160 bytes, and access is exclusive.
        let input = pmm::alloc_zero()? as *mut u8;
        let interval = encode_interval((*g).dev_speed, s.interval);
        // SAFETY: in-bounds per the allocation contract above.
        let ic = &mut *(input as *mut ctx::InputCtx);
        ic.add_flags = 1 | (1u32 << dci);
        // SAFETY: slot context at offset 32, in bounds per contract.
        let sc = &mut *(input.add(32) as *mut ctx::SlotCtx);
        ctx::slot_ctx_root_port(sc, (*g).dev_port, (*g).dev_speed, dci);
        let words = intr_ep_words(interval, s.max_packet, ring_pa, dcs);
        // SAFETY: EP context at offset 32 + dci*32 (dci ≤ 31), in bounds.
        let ep = &mut *(input.add(32 + dci as usize * 32) as *mut ctx::EpCtx);
        ep.dw0 = words[0];
        ep.dw1 = words[1];
        ep.dw2 = words[2];
        ep.dw3 = words[3];
        ep.dw4 = words[4];

        // Configure Endpoint (TRB type 12) via the shared command ring.
        let mut trb = ring::Trb::zero();
        trb.params[0] = input as u32;
        trb.params[1] = ((input as u64) >> 32) as u32;
        trb.set_type(ring::TRB_CONFIG_EP);
        trb.set_flags(ring::TRB_IOC);
        // Slot id rides in the control word (§6.4.3.4), after set_type.
        trb.set_slot_id(slot_id);
        let ev = ring::submit_command(&trb)?;
        if (ev.params[2] >> 24) & 0xFF != CC_SUCCESS {
            return Err(Errno::Io);
        }

        // Pre-post N_BUFS buffers, then a single doorbell runs them all.
        let bufs_pa = pmm::alloc_zero()? as u64;
        for i in 0..N_BUFS {
            let buf_pa = bufs_pa + (i as u64) * u64::from(s.max_packet);
            let trb = intr_buf_trb(buf_pa, u32::from(s.max_packet));
            // SAFETY: ring_ptr holds the fully initialized interrupt ring.
            ring::enqueue_trb(&mut *ring_ptr, &trb);
        }
        ring::ring_doorbell(slot_id, dci);

        // SAFETY: single-owner boot path; the state written is fully
        // consistent (endpoint configured, pool posted).
        super::G_INTR = IntrState {
            armed: true,
            dci,
            slot_id,
            max_pkt: s.max_packet,
            ring_pa,
            bufs_pa,
        };
        crate::kinf!(
            "xhci",
            "intr armed dci=%d mps=%d interval=%d",
            Arg::from(dci as u32),
            Arg::from(s.max_packet as u32),
            Arg::from(u32::from(interval))
        );
        Ok(())
    }
}

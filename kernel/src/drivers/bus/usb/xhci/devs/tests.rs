//! Routing, claim and release tests for the per-device table — the
//! multi-device invariants: events reach only their own device, a slot
//! or port is never double-claimed, release frees every owned page.

use super::*;
use crate::drivers::usb::xhci::ring::TRB_TRANSFER_EVENT;

/// A transfer event for `slot`/`dci` (DW3: type, DCI, slot id).
fn xfer_ev(slot: u8, dci: u8, trb_ptr: u32) -> Trb {
    let mut t = Trb::zero();
    t.params[0] = trb_ptr;
    t.params[3] = (u32::from(slot) << 24) | (u32::from(dci) << 16) | (TRB_TRANSFER_EVENT << 10);
    t
}

fn table() -> [XhciDev; MAX_DEVS] {
    [const { XhciDev::new() }; MAX_DEVS]
}

fn arm(d: &mut XhciDev, dci: u8) {
    d.intr.armed = true;
    d.intr.dci = dci;
}

#[test]
fn event_fields_decode_slot_and_dci() {
    let ev = xfer_ev(5, 3, 0x1000);
    assert_eq!(event_slot(&ev), 5);
    assert_eq!(event_dci(&ev), 3);
}

#[test]
fn events_are_parked_on_the_owning_device_only() {
    let mut t = table();
    claim(&mut t, 1, 0, 1).unwrap();
    claim(&mut t, 2, 1, 1).unwrap();
    arm(&mut t[0], 3);
    arm(&mut t[1], 3);
    // Same DCI on both devices: only the slot id tells them apart.
    assert!(route_park(&mut t, &xfer_ev(2, 3, 0xB000)));
    assert!(route_park(&mut t, &xfer_ev(1, 3, 0xA000)));
    assert_eq!(t[0].intr.take_parked().unwrap().params[0], 0xA000);
    assert!(t[0].intr.take_parked().is_none());
    assert_eq!(t[1].intr.take_parked().unwrap().params[0], 0xB000);
    assert!(t[1].intr.take_parked().is_none());
}

#[test]
fn unroutable_events_are_not_parked() {
    let mut t = table();
    claim(&mut t, 1, 0, 1).unwrap();
    arm(&mut t[0], 3);
    // Slot 0, out-of-range slot, dead slot, wrong endpoint.
    assert!(!route_park(&mut t, &xfer_ev(0, 3, 1)));
    assert!(!route_park(&mut t, &xfer_ev(200, 3, 1)));
    assert!(!route_park(&mut t, &xfer_ev(2, 3, 1)));
    assert!(!route_park(&mut t, &xfer_ev(1, 5, 1)));
    assert!(t[0].intr.take_parked().is_none());
}

#[test]
fn park_fifo_is_bounded_and_fifo_ordered() {
    let mut t = table();
    claim(&mut t, 1, 0, 1).unwrap();
    arm(&mut t[0], 3);
    for i in 0..N_BUFS as u32 {
        assert!(route_park(&mut t, &xfer_ev(1, 3, 0x100 + i)));
    }
    assert!(!route_park(&mut t, &xfer_ev(1, 3, 0xFFFF)));
    for i in 0..N_BUFS as u32 {
        assert_eq!(t[0].intr.take_parked().unwrap().params[0], 0x100 + i);
    }
}

#[test]
fn claim_rejects_bad_ids_busy_slots_and_shared_ports() {
    let mut t = table();
    assert_eq!(claim(&mut t, 0, 0, 1).err(), Some(Errno::Range));
    assert_eq!(
        claim(&mut t, MAX_DEVS as u8 + 1, 0, 1).err(),
        Some(Errno::Range)
    );
    claim(&mut t, 1, 4, 1).unwrap();
    assert_eq!(claim(&mut t, 1, 5, 1).err(), Some(Errno::Busy));
    assert_eq!(claim(&mut t, 2, 4, 1).err(), Some(Errno::Busy));
    claim(&mut t, 2, 5, 3).unwrap();
    assert_eq!((t[1].port, t[1].speed), (5, 3));
}

#[test]
fn release_frees_all_pages_and_reopens_the_slot() {
    let mut t = table();
    let d = claim(&mut t, 3, 2, 1).unwrap();
    let mut next = 0x1000u64;
    for _ in 0..3 {
        d.pages
            .take(
                || {
                    next += 0x1000;
                    Ok(next)
                },
                |_| {},
            )
            .unwrap();
    }
    arm(d, 3);
    let mut freed = 0;
    d.release(|_| freed += 1);
    assert_eq!(freed, 3);
    assert!(!t[2].in_use && !t[2].intr.armed);
    assert!(t[2].ring_mut(1).is_none());
    // The slot and the port are reusable after release.
    claim(&mut t, 3, 2, 1).unwrap();
}

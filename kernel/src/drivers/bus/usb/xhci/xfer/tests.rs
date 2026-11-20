use super::{DCI_EP0, completion_bytes, control_trbs, event_completes};
use crate::drivers::usb::xhci::ring::{Trb, setup_trb};
use onyx_core::errno::Errno;

/// DW3 layout check: type at 10..16, CH bit 4, IOC bit 5, IDT bit 6,
/// direction/TRT at bit 16.
fn dw3(trb: &Trb) -> u32 {
    trb.params[3]
}

#[test]
fn control_td_chains_setup_data_status_with_the_right_dirs() {
    // GET_DESCRIPTOR IN, wValue=0x0100 (descriptor type in the high byte,
    // index in the low byte, USB 2.0 §9.4.3), wLength=18 — the exact wire
    // bytes `requests::get_descriptor` emits, little-endian fields.
    let setup = [0x80u8, 6, 0x00, 0x01, 0x00, 0x00, 0x12, 0x00];
    let (trbs, n) = unsafe { control_trbs(&setup, 0x8000_1000, 18, true) };
    assert_eq!(n, 3);
    // Setup TRB DW0/DW1 carry the packet's wire bytes: DW0 packs
    // bmRequestType | bRequest<<8 | wValue<<16, so wValue must read back
    // as 0x0100 (type in the high byte) — not byte-swapped.
    assert_eq!(trbs[0].params[0], 0x80 | (6u32 << 8) | (0x0100u32 << 16));
    // DW1: wIndex=0 in the low half, wLength=18 in the high half.
    assert_eq!(trbs[0].params[1], 18u32 << 16);
    // Setup: type=2, CH (data+status follow), TRT=3 (IN data stage).
    assert_eq!((dw3(&trbs[0]) >> 10) & 0x3F, 2);
    assert_ne!(dw3(&trbs[0]) & (1 << 4), 0);
    assert_eq!((dw3(&trbs[0]) >> 16) & 3, 3);
    // Data: type=3, CH, DIR=IN, length in DW2, buffer pointer split.
    assert_eq!((dw3(&trbs[1]) >> 10) & 0x3F, 3);
    assert_ne!(dw3(&trbs[1]) & (1 << 4), 0);
    assert_ne!(dw3(&trbs[1]) & (1 << 16), 0);
    assert_eq!(trbs[1].params[2], 18);
    assert_eq!(trbs[1].params[0], 0x8000_1000u32);
    // Status: type=4, IOC, DIR=OUT (opposite the IN data).
    assert_eq!((dw3(&trbs[2]) >> 10) & 0x3F, 4);
    assert_ne!(dw3(&trbs[2]) & (1 << 5), 0);
    assert_eq!(dw3(&trbs[2]) & (1 << 16), 0);
}

#[test]
fn no_data_control_stops_after_an_in_status() {
    // SET_ADDRESS, wLength=0 → no Data Stage, Status is IN.
    let setup = [0x00u8, 5, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00];
    let (trbs, n) = unsafe { control_trbs(&setup, 0, 0, false) };
    assert_eq!(n, 2);
    assert_eq!((dw3(&trbs[1]) >> 10) & 0x3F, 4);
    assert_ne!(dw3(&trbs[1]) & (1 << 5), 0);
    assert_ne!(dw3(&trbs[1]) & (1 << 16), 0);
    // Setup TRT=0 (no data stage follows).
    assert_eq!((dw3(&trbs[0]) >> 16) & 3, 0);
}

#[test]
fn setup_trb_marks_immediate_data() {
    let mut trb = Trb::zero();
    // SAFETY: pure field arithmetic on a stack Trb.
    unsafe { setup_trb(&mut trb, 6, 0x80, 0x0100, 0, 18) };
    assert_eq!(trb.params[0], 0x80 | (6 << 8) | (0x0100 << 16));
    assert_eq!(trb.params[1], 18 << 16);
    assert_eq!(trb.params[2], 8); // transfer length = the 8-byte packet
    assert_ne!(dw3(&trb) & (1 << 6), 0); // IDT
}

#[test]
fn completion_maps_residual_and_rejects_errors() {
    // Success, residual 10 of 64 → 54 bytes moved.
    let mut ev = Trb::zero();
    ev.params[2] = (1 << 24) | 10;
    assert_eq!(completion_bytes(&ev, 64), Ok(54));
    // Short Packet (13) is a completion, not an error.
    ev.params[2] = (13 << 24) | 56;
    assert_eq!(completion_bytes(&ev, 64), Ok(8));
    // Stall (6) surfaces as Io.
    ev.params[2] = 6 << 24;
    assert_eq!(completion_bytes(&ev, 64), Err(Errno::Io));
}

/// Transfer-event DW3: slot id at [31:24], endpoint id at [20:16], TRB
/// type at [15:10], cycle at bit 0 (§6.4.2.1).
fn transfer_event(slot: u32, epid: u32) -> Trb {
    let mut ev = Trb::zero();
    ev.params[3] = (slot << 24) | (epid << 16) | (32 << 10) | 1;
    ev
}

#[test]
fn event_filter_separates_control_and_interrupt_waits() {
    // A control completion (EP0, DCI 1) satisfies a control wait only —
    // never an interrupt wait (ruling 1: one shared event ring).
    let ctl = transfer_event(1, 1);
    assert!(event_completes(&ctl, DCI_EP0));
    assert!(!event_completes(&ctl, 3));
    // An interrupt completion (EP1 IN, DCI 3) satisfies an interrupt
    // wait only — never the control wait.
    let intr = transfer_event(1, 3);
    assert!(event_completes(&intr, 3));
    assert!(!event_completes(&intr, DCI_EP0));
}

#[test]
fn event_filter_ignores_command_and_port_events() {
    // Command completions (the bringup paths' currency) and port status
    // changes satisfy no transfer wait: they are consumed and dropped.
    let mut ev = Trb::zero();
    ev.params[3] = (33 << 10) | 1; // COMMAND COMPLETE, slot 1, EP0 fields set
    assert!(!event_completes(&ev, DCI_EP0));
    assert!(!event_completes(&ev, 3));
    ev.params[3] = (34 << 10) | 1; // PORT STATUS CHANGE
    assert!(!event_completes(&ev, DCI_EP0));
    assert!(!event_completes(&ev, 3));
}

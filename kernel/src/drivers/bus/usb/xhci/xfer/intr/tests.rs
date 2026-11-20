//! Host tests for the interrupt-IN pure logic: the DCI mapping, the
//! spec interval encoding (§6.2.3 Table 6-12), byte-exact buffer TRBs
//! and the endpoint-context word layout. No MMIO is touched — the arm
//! and poll drivers are single-owner paths over controller registers.

use super::arm::{encode_interval, intr_ep_words};
use super::{ep_dci, intr_buf_trb};
use crate::drivers::usb::xhci::device::{SPEED_FS, SPEED_HS, SPEED_LS, SPEED_SS};
use crate::drivers::usb::xhci::ring;

#[test]
fn dci_maps_in_endpoints_only() {
    // DCI = 2*ep_num + 1 for IN (§4.5.1): EP1 IN → 3, EP2 IN → 5.
    assert_eq!(ep_dci(0x81), Some(3));
    assert_eq!(ep_dci(0x82), Some(5));
    assert_eq!(ep_dci(0x8F), Some(31));
    // OUT endpoints and EP0 never carry interrupt-IN polls.
    assert_eq!(ep_dci(0x01), None);
    assert_eq!(ep_dci(0x00), None);
}

#[test]
fn interval_encoding_follows_speed_rules() {
    // FS/LS: bInterval is frames (1..255); the context field is
    // floor(log2(bInterval)) + 3, clamped to 3..10 (Table 6-12).
    assert_eq!(encode_interval(SPEED_FS, 1), 3);
    assert_eq!(encode_interval(SPEED_FS, 8), 6);
    assert_eq!(encode_interval(SPEED_FS, 10), 6); // 10 ms rounds down to 8 ms
    assert_eq!(encode_interval(SPEED_FS, 255), 10);
    assert_eq!(encode_interval(SPEED_LS, 100), 9); // rounds to 64 ms
    // HS/SS: bInterval is 2^(b-1) microframes; the field is just b-1.
    assert_eq!(encode_interval(SPEED_HS, 1), 0);
    assert_eq!(encode_interval(SPEED_HS, 4), 3); // 1 ms
    assert_eq!(encode_interval(SPEED_HS, 16), 15);
    assert_eq!(encode_interval(SPEED_SS, 4), 3);
}

#[test]
fn intr_buf_trb_is_byte_exact_normal_with_ioc() {
    let t = intr_buf_trb(0x1234_5678_9ABC_DEF0, 64);
    // Buffer pointer split across DW0/DW1, transfer length in DW2.
    assert_eq!(t.params[0], 0x9ABC_DEF0);
    assert_eq!(t.params[1], 0x1234_5678);
    assert_eq!(t.params[2], 64);
    // DW3: type=NORMAL at bits [15:10], IOC at bit 5, cycle bit left
    // for enqueue_trb, interrupter target (bits [31:22]) zero.
    assert_eq!((t.params[3] >> 10) & 0x3F, ring::TRB_NORMAL);
    assert_ne!(t.params[3] & ring::TRB_IOC, 0);
    assert_eq!(t.params[3] & ring::TRB_C, 0);
    assert_eq!(t.params[3] >> 22, 0);
}

#[test]
fn ep_ctx_words_follow_spec_6_2_3() {
    let w = intr_ep_words(6, 8, 0x8000_0000, true);
    // DW0: Interval at bits [23:16]; EP State/Mult/MaxPStreams/LSA zero.
    assert_eq!(w[0], 6 << 16);
    // DW1: MaxPkt at [31:16], EP Type 7 (Interrupt IN) at [7:5], CErr 3.
    assert_eq!(w[1], 8 << 16 | 7 << 3 | 3 << 1);
    // DW2/DW3: dequeue pointer, DCS at DW2 bit 0.
    assert_eq!(w[2], 0x8000_0001);
    assert_eq!(w[3], 0);
    // DW4: Average TRB Length = mps, Max ESIT Payload = mps (burst 0).
    assert_eq!(w[4], 8 | 8 << 16);
}

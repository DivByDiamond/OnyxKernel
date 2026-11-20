//! Spec-conformance checks for the OHCI ED/TD bit-field constants: every
//! expected value below is the OHCI 1.0a spec layout (§4.2.2 ED word 0,
//! §4.3.1 general TD word 0), cross-checked against Linux
//! `drivers/usb/host/ohci.h` and QEMU `hw/usb/hcd-ohci.c` — which agree
//! with each other bit for bit.

use super::*;

#[test]
fn td_condition_code_lives_in_high_nibble() {
    // Linux ohci.h: TD_CC = 0xf0000000, TD_NOTACCESSED = 0x0F;
    // QEMU hcd-ohci.c: OHCI_TD_CC_SHIFT 28.
    assert_eq!(TD_CC_MASK, 0xF000_0000);
    assert_eq!(TD_CC_NOT_ACCESSED, 0xF000_0000);
}

#[test]
fn td_data_toggle_lives_in_bits_24_25() {
    // Linux ohci.h: TD_T_DATA0 0x02000000, TD_T_DATA1 0x03000000 — the
    // T field is bits 24-25, not bits 10-11.
    assert_eq!(TD_T_DATA0, 0x0200_0000);
    assert_eq!(TD_T_DATA1, 0x0300_0000);
}

#[test]
fn td_direction_delay_and_rounding_positions() {
    // Linux ohci.h: TD_DP_IN 0x00100000, TD_DP_OUT 0x00080000,
    // TD_R 0x00040000, TD_DI_SET(7) = 0x00E00000 — DP is bits 19-20,
    // DI bits 21-23, R bit 18.
    assert_eq!(TD_DP_SETUP, 0x0000_0000);
    assert_eq!(TD_DP_OUT, 0x0008_0000);
    assert_eq!(TD_DP_IN, 0x0010_0000);
    assert_eq!(TD_R_ROUND, 0x0004_0000);
    assert_eq!(TD_DI_NO_INTR, 0x00E0_0000);
}

#[test]
fn control_td_words_match_spec_composites() {
    // The TD control words ohci_control_transfer builds (§4.3.1):
    // setup = DATA0/SETUP, data-IN = DATA1/IN, status-after-IN =
    // DATA1/OUT — all CC=NOT_ACCESSED, no interrupt, buffer rounding.
    let setup = TD_CC_NOT_ACCESSED | TD_T_DATA0 | TD_DI_NO_INTR | TD_DP_SETUP | TD_R_ROUND;
    assert_eq!(setup, 0xF2E4_0000);
    let data_in = TD_CC_NOT_ACCESSED | TD_T_DATA1 | TD_DI_NO_INTR | TD_DP_IN | TD_R_ROUND;
    assert_eq!(data_in, 0xF3F4_0000);
    let status_after_in = TD_CC_NOT_ACCESSED | TD_T_DATA1 | TD_DI_NO_INTR | TD_DP_OUT | TD_R_ROUND;
    assert_eq!(status_after_in, 0xF3EC_0000);
}

#[test]
fn ed_speed_bit_is_bit_13_not_direction() {
    // Linux ohci.h: ED_LOWSPEED = 1 << 13. Bit 12 is the D direction
    // field's high bit — the pre-fix 1<<12 encoding set D=IN instead of
    // marking the device low speed.
    assert_eq!(ED_SPEED_FULL, 0);
    assert_eq!(ED_SPEED_LOW, 1 << 13);
}

#[test]
fn ed_word_composites_match_spec() {
    // §4.2.2 field table: FA bits 0-6, MPS bits 16-26 (QEMU
    // OHCI_ED_MPS_SHIFT 16), speed bit per the test above.
    assert_eq!(
        (0x25u32 << ED_FA_SHIFT) | ED_SPEED_FULL | (64 << ED_MPS_SHIFT),
        0x0040_0025
    );
    assert_eq!(
        (3u32 << ED_FA_SHIFT) | ED_SPEED_LOW | (8 << ED_MPS_SHIFT),
        0x0008_2003
    );
}

#[test]
fn hc_control_rwe_is_bit_10() {
    // Linux ohci.h: OHCI_CTRL_RWE = 1 << 10 — bit 9 is RWC (remote
    // wakeup connected), a read-only status bit.
    assert_eq!(OHCI_CTRL_RWE, 1 << 10);
}

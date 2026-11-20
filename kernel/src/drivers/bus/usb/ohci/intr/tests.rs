//! Byte-exact host tests of the pure OHCI interrupt builders (ruling 6:
//! no MMIO anywhere in these) — every expected value is the spec 1.0a
//! layout cross-checked with Linux `drivers/usb/host/ohci.h` and QEMU
//! `hw/usb/hcd-ohci.c`.

use super::*;

/// ED format bit F, bit 15 (spec 1.0a §4.2.2): 0 = general TDs — the
/// value an interrupt ED must carry not to be read as isochronous.
const ED_F_ISO: u32 = 1 << 15;

#[test]
fn intr_ed_fields_pack_fa_en_and_mps() {
    // FS device 0x25, interrupt EP 1, MPS 64: FA = 0x25 (bits 0-6),
    // EN = 1 (bits 7-10), S = 0 (bit 13, full), K = 0, F = 0,
    // MPS = 64<<16 (bits 16-26; QEMU OHCI_ED_MPS_SHIFT 16).
    assert_eq!(intr_ed_fields(0x25, 0x01, 64), 0x0040_00A5);
}

#[test]
fn intr_ed_fields_mask_addr_and_endpoint() {
    // Only the low 7 bits of the address and the low 4 of the endpoint
    // number land in the fields (QEMU masks OHCI_ED_FA_MASK 0x7f,
    // OHCI_ED_EN_MASK 0xf<<7) — the IN-direction bit of ep_in drops off.
    assert_eq!(intr_ed_fields(0xFF, 0x81, 8), intr_ed_fields(0x7F, 0x01, 8));
}

#[test]
fn intr_ed_fields_format_is_zero() {
    // F (bit 15) clear: interrupt transfers ride general TDs — an
    // isochronous-format ED would make the HC read the TD as iso.
    for (a, e) in [(0u8, 0u8), (0x7F, 0x0F)] {
        assert_eq!(intr_ed_fields(a, e, 64) & ED_F_ISO, 0);
    }
}

#[test]
fn intr_ed_fields_clamps_mps_to_field_width() {
    // MPS is 11 bits (0x7ff<<16): a hostile descriptor value cannot
    // overflow into the reserved bits above the field.
    assert_eq!(intr_ed_fields(1, 1, 0xFFFF), intr_ed_fields(1, 1, 0x3FF));
}

#[test]
fn intr_td_fields_is_in_not_accessed_rounded_carry() {
    // DP = 10b IN (bits 19-20, Linux TD_DP_IN 0x00100000), CC = 0xF
    // NOT_ACCESSED (bits 28-31, Linux TD_NOTACCESSED), R = 1 (bit 18,
    // Linux TD_R: a short report is fine), DI = 7 no interrupt (bits
    // 21-23), T = 00b from the ED carry (bits 24-25) — no DATA0 bit.
    assert_eq!(intr_td_fields(), 0xF0F4_0000);
}

#[test]
fn intr_list_indices_clamps_interval_to_list_wheel() {
    // One HCCA list per 1-ms frame: stride = clamped bInterval.
    // bInterval 0 is invalid and degrades to 1 ms; anything over the
    // 32-list wheel polls at 32 ms — never faster than asked.
    assert_eq!(intr_list_indices(1), (0, 1));
    assert_eq!(intr_list_indices(10), (0, 10));
    assert_eq!(intr_list_indices(32), (0, 32));
    assert_eq!(intr_list_indices(255), (0, 32));
    assert_eq!(intr_list_indices(0), (0, 1));
}

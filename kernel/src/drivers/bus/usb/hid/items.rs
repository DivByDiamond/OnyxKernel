//! HID item grammar and bit-level codecs (HID 1.11 §6.2.2, §5.8).
//!
//! A short item's first byte packs bits 0-1 data size (0/1/2/4 bytes),
//! bits 2-3 item type, bits 4-7 tag (§6.2.2.2); long items take the
//! 0xFE prefix (§6.2.2.3). Report fields are little-endian with the
//! least significant bit in bit 0 (§5.8).

/// Item type: the extracted bits 2-3 of the item byte (§6.2.2.2).
pub(super) const TYPE_MAIN: u8 = 0x00;
pub(super) const TYPE_GLOBAL: u8 = 0x01;
pub(super) const TYPE_LOCAL: u8 = 0x02;

/// Item tags: the upper nibble of the item byte.
pub(super) const TAG_USAGE: u8 = 0x00; // Local (§6.2.2.8)
pub(super) const TAG_USAGE_MIN: u8 = 0x10; // Local
pub(super) const TAG_USAGE_MAX: u8 = 0x20; // Local
pub(super) const TAG_INPUT: u8 = 0x80; // Main (§6.2.2.4)
pub(super) const TAG_OUTPUT: u8 = 0x90; // Main
pub(super) const TAG_COLLECTION: u8 = 0xA0; // Main
pub(super) const TAG_FEATURE: u8 = 0xB0; // Main
pub(super) const TAG_END: u8 = 0xC0; // Main
pub(super) const TAG_USAGE_PAGE: u8 = 0x00; // Global (§6.2.2.7)
pub(super) const TAG_LOGICAL_MIN: u8 = 0x10; // Global
pub(super) const TAG_LOGICAL_MAX: u8 = 0x20; // Global
pub(super) const TAG_PHYSICAL_MIN: u8 = 0x30; // Global (§6.2.2.7)
pub(super) const TAG_PHYSICAL_MAX: u8 = 0x40; // Global
pub(super) const TAG_REPORT_SIZE: u8 = 0x70; // Global
pub(super) const TAG_REPORT_ID: u8 = 0x80; // Global
pub(super) const TAG_REPORT_COUNT: u8 = 0x90; // Global

/// Long-item prefix (§6.2.2.3): 0xFE, bDataSize, bTag, data.
pub(super) const LONG_PREFIX: u8 = 0xFE;

/// Short-item data size code → bytes (§6.2.2.2: code 3 encodes 4).
pub(super) fn short_size(code: u8) -> usize {
    match code & 0x03 {
        0 => 0,
        1 => 1,
        2 => 2,
        _ => 4,
    }
}

/// Item data as a two's-complement integer, sign-extended from the
/// data's own width (§5.8 — global values are signed).
pub(super) fn read_signed(data: &[u8]) -> i32 {
    let mut v: u32 = 0;
    for (i, &byte) in data.iter().enumerate() {
        v |= u32::from(byte) << (8 * i); // ≤ 4 data bytes per item
    }
    let width = 8 * data.len();
    if width > 0 && width < 32 && (v >> (width - 1)) & 1 == 1 {
        v |= !((1u32 << width) - 1);
    }
    v as i32
}

/// Extract `bits` (≤ 16 significant) LSB-first at `off_bits`: report
/// fields store the least significant bit in bit 0 (§5.8). Byte reads
/// stay bounds-checked against the actual report.
pub(super) fn extract_bits(report: &[u8], off_bits: u32, bits: u8) -> u16 {
    let mut v = 0u16;
    for i in 0..u32::from(bits).min(16) {
        if let Some(bit) = off_bits.checked_add(i) {
            let byte = (bit / 8) as usize;
            if byte < report.len() && report[byte] & (1u8 << (bit % 8)) != 0 {
                v |= 1u16 << i;
            }
        }
    }
    v
}

/// Raw field value: sign-extend from the field's own width when the
/// logical bounds were signed (ruling 5, §5.8); unsigned values beyond
/// the i16 range saturate; wider-than-16 fields decode their low half.
pub(super) fn raw_value(raw: u16, signed: bool, bits: u8) -> i16 {
    let width = bits.min(16);
    if width == 0 || !signed {
        return if raw > i16::MAX as u16 {
            i16::MAX
        } else {
            raw as i16
        };
    }
    if width == 16 {
        return raw as i16;
    }
    // Arithmetic shifts sign-extend from `width`.
    ((raw << (16 - width)) as i16) >> (16 - width)
}

/// Hat-switch nibble → (dx, dy), joydev ABS_HAT0X/0Y semantics (ruling
/// 2): 0 null, 1..8 = N,NE,E,SE,S,SW,W,NW; 9..15 out of range → null.
/// Index 5 (S) → (0, 1), the brief's decode fixture.
pub(super) const HAT_AXES: [(i8, i8); 16] = [
    (0, 0),
    (0, -1),
    (1, -1),
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
    (-1, -1),
    (0, 0),
    (0, 0),
    (0, 0),
    (0, 0),
    (0, 0),
    (0, 0),
    (0, 0),
];

/// Hat nibble masked to the 16-entry position table.
pub(super) fn hat_index(raw: u16) -> usize {
    (raw & 0x0F) as usize
}

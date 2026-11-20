//! HID report-descriptor parser and input-report decoder.
//!
//! [`parse_report_desc`] walks short items per HID 1.11 §6.2.2 (long
//! items skipped, §6.2.2.3) and builds a fixed [`HidTable`] through
//! the §5.4 item-state table in [`state`]. [`decode_report`] is
//! allocation-free: it fills a caller-provided array (ruling 6).

use super::items::{
    HAT_AXES, LONG_PREFIX, TAG_COLLECTION, TAG_END, TAG_FEATURE, TAG_INPUT, TAG_LOGICAL_MAX,
    TAG_LOGICAL_MIN, TAG_OUTPUT, TAG_PHYSICAL_MAX, TAG_PHYSICAL_MIN, TAG_REPORT_COUNT,
    TAG_REPORT_ID, TAG_REPORT_SIZE, TAG_USAGE, TAG_USAGE_MAX, TAG_USAGE_MIN, TAG_USAGE_PAGE,
    TYPE_GLOBAL, TYPE_LOCAL, TYPE_MAIN, extract_bits, hat_index, raw_value, read_signed,
    short_size,
};
use super::state::State;
use super::{EMPTY_FIELD, FieldKind, HatPart, HidError, HidTable, MAX_FIELDS};

/// Parse a report descriptor into a decode table. Supported items:
/// Input/Output/Feature/Collection/End, Usage Page, Logical Min/Max,
/// Physical Min/Max (parsed and dropped — decode is logical-valued),
/// Report Size/Count, Usage and Usage Min/Max; long items are skipped
/// (§6.2.2.3); anything else is `UnknownItem`. Report IDs are rejected
/// outright (ruling 4). `Err(NotStick)` when no axis emerged
/// (js-criterion).
pub fn parse_report_desc(desc: &[u8]) -> Result<HidTable, HidError> {
    let mut st = State::new();
    let mut table = HidTable {
        fields: [EMPTY_FIELD; MAX_FIELDS],
        n_fields: 0,
        report_len_bits: 0,
    };
    let mut i = 0usize;
    while i < desc.len() {
        let b = desc[i];
        if b == LONG_PREFIX {
            // 0xFE, bDataSize, bLongItemTag, data — vendor tags, skipped.
            let size = match desc.get(i.checked_add(1).ok_or(HidError::Short)?) {
                Some(&s) => s,
                None => return Err(HidError::Short),
            };
            let end = i
                .checked_add(3)
                .and_then(|e| e.checked_add(size as usize))
                .ok_or(HidError::Short)?;
            if end > desc.len() {
                return Err(HidError::Short);
            }
            i = end;
            continue;
        }
        // Short item (§6.2.2.2): size in bits 0-1, type in bits 2-3,
        // tag in bits 4-7.
        let size = short_size(b);
        let typ = (b >> 2) & 0x03;
        let tag = b & 0xF0;
        let end = i
            .checked_add(1)
            .and_then(|e| e.checked_add(size))
            .ok_or(HidError::Short)?;
        if end > desc.len() {
            return Err(HidError::Short);
        }
        let val = read_signed(&desc[i + 1..end]);
        match (typ, tag) {
            (TYPE_GLOBAL, TAG_USAGE_PAGE) => {
                // Pages are 16-bit (§6.2.2.7); malformed widths keep the
                // current page rather than guessing.
                if let Ok(p) = u16::try_from(val) {
                    st.page = p;
                }
            }
            (TYPE_GLOBAL, TAG_LOGICAL_MIN) => st.logical_min = val,
            (TYPE_GLOBAL, TAG_LOGICAL_MAX) => {
                // Parsed for grammar compatibility; decode reads only
                // the minimum (ruling 5).
            }
            (TYPE_GLOBAL, TAG_PHYSICAL_MIN) | (TYPE_GLOBAL, TAG_PHYSICAL_MAX) => {
                // Same stance as Logical Max: the physical range only
                // rescales values for human units (§6.2.2.7) and the
                // decode uses logical values — QEMU's usb-tablet and
                // plenty of real sticks carry 0x35/0x46 items.
            }
            (TYPE_GLOBAL, TAG_REPORT_SIZE) => st.bits = val.clamp(0, u8::MAX as i32) as u8,
            (TYPE_GLOBAL, TAG_REPORT_COUNT) => st.count = val.clamp(0, u8::MAX as i32) as u8,
            (TYPE_GLOBAL, TAG_REPORT_ID) => return Err(HidError::ReportId),
            (TYPE_LOCAL, TAG_USAGE) => st.add_usage(val)?,
            (TYPE_LOCAL, TAG_USAGE_MIN) => st.set_usage_min(val),
            (TYPE_LOCAL, TAG_USAGE_MAX) => st.set_usage_max(val),
            (TYPE_MAIN, TAG_INPUT) => st.do_input(&mut table)?,
            (TYPE_MAIN, TAG_OUTPUT) | (TYPE_MAIN, TAG_FEATURE) => {
                // Output/Feature reports keep their own offsets; only
                // the input report is decoded — parse and drop (§5.4).
            }
            (TYPE_MAIN, TAG_COLLECTION) | (TYPE_MAIN, TAG_END) => {
                // Grouping only; globals persist (§5.4).
            }
            _ => return Err(HidError::UnknownItem(b)),
        }
        if typ == TYPE_MAIN {
            st.clear_locals();
        }
        i = end;
    }
    table.report_len_bits = u16::try_from(st.offset()).map_err(|_| HidError::TooManyFields)?;
    if st.axes_seen() == 0 {
        return Err(HidError::NotStick);
    }
    Ok(table)
}

/// Decode one input report against `t`: fills `out[0..n]` with each
/// field's `(kind, value)`, returns `n`. A report shorter than the
/// table's byte extent decodes nothing — the caller retries with the
/// full report; the `usize` return is the interface's no-allocation
/// contract (truncation inside a descriptor is the parse-side
/// `HidError::Short`).
pub fn decode_report(
    t: &HidTable,
    report: &[u8],
    out: &mut [(FieldKind, i16); MAX_FIELDS],
) -> usize {
    let need = usize::from(t.report_len_bits).div_ceil(8);
    if report.len() < need {
        return 0;
    }
    let mut n = 0usize;
    for f in t.fields.iter().take(t.n_fields) {
        let raw = extract_bits(report, u32::from(f.offset_bits), f.bits);
        let value = match f.hat {
            HatPart::HatX => i16::from(HAT_AXES[hat_index(raw)].0),
            HatPart::HatY => i16::from(HAT_AXES[hat_index(raw)].1),
            HatPart::NotHat => raw_value(raw, f.signed, f.bits),
        };
        // n < n_fields ≤ MAX_FIELDS keeps this live; the match stays
        // panic-free for any table.
        match out.get_mut(n) {
            Some(slot) => {
                *slot = (f.kind, value);
                n += 1;
            }
            None => break,
        }
    }
    n
}

#[cfg(test)]
mod decode_tests;
#[cfg(test)]
pub(crate) mod fixtures;
#[cfg(test)]
mod tests;

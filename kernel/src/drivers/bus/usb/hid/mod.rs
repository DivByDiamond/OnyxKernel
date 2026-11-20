//! USB HID class support: report-descriptor parsing and report decoding.
//!
//! A HID device describes its own data with a report descriptor (HID
//! 1.11 §6.2.2); [`report::parse_report_desc`] turns that byte string
//! into a fixed [`HidTable`] of axis/button fields, and
//! [`report::decode_report`] maps wire report bytes onto it. Pure
//! data-plane logic — no MMIO, no allocation, no unsafe — so the whole
//! path is host-testable. Usage values follow the HID Usage Tables:
//! Generic Desktop page 0x01 (X 0x30 .. Slider 0x36, Hat switch 0x39)
//! and Button page 0x09 (usage N names button N).

pub mod items;
pub mod report;
pub mod session;
mod state;

pub use report::{decode_report, parse_report_desc};
pub use session::{Baseline, HidEvent, attach, baseline, poll, stick_slots};

/// GET_DESCRIPTOR type for a HID report descriptor (HID 1.11 §7.1).
pub const HID_DESC_REPORT: u8 = 0x22;

/// Fixed decode-table capacity: one slot per axis/button field.
pub const MAX_FIELDS: usize = 64;

/// Generic Desktop usage page (HID Usage Tables, page 0x01).
pub const PAGE_GENERIC_DESKTOP: u16 = 0x01;
/// Button usage page (HID Usage Tables, page 0x09).
pub const PAGE_BUTTON: u16 = 0x09;

/// Generic Desktop axis usages (HID Usage Tables §4).
pub const GD_X: u16 = 0x30;
pub const GD_Y: u16 = 0x31;
pub const GD_Z: u16 = 0x32;
pub const GD_RX: u16 = 0x33;
pub const GD_RY: u16 = 0x34;
pub const GD_RZ: u16 = 0x35;
pub const GD_SLIDER: u16 = 0x36;
/// Hat switch (0x39): decodes as two -1/0/1 axes (ruling 2).
pub const GD_HAT: u16 = 0x39;

/// Which logical control a report field feeds. Axis ids follow the
/// first-appearance order in the descriptor; button ids are the
/// Button-page usage number minus one (ruling 3).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum FieldKind {
    Axis(u8),
    Button(u8),
}

/// Hat-switch component of a field: the raw nibble decodes through the
/// 16-entry position table instead of raw extraction (ruling 2).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum HatPart {
    NotHat,
    HatX,
    HatY,
}

/// One decoded input-report field: control, bit width, bit offset,
/// signedness (a field is signed iff its logical minimum is negative —
/// ruling 5, HID 1.11 §5.8) and hat component.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ReportField {
    pub kind: FieldKind,
    pub bits: u8,
    pub offset_bits: u16,
    /// Signed logical bounds: the raw bits sign-extend on decode.
    pub signed: bool,
    /// Hat component; `NotHat` marks a plain raw field.
    pub hat: HatPart,
}

/// Template for unused table slots (bounded by `n_fields`).
pub(crate) const EMPTY_FIELD: ReportField = ReportField {
    kind: FieldKind::Axis(0),
    bits: 0,
    offset_bits: 0,
    signed: false,
    hat: HatPart::NotHat,
};

/// Parsed decode table for one input report.
#[derive(PartialEq, Debug)]
pub struct HidTable {
    pub fields: [ReportField; MAX_FIELDS],
    /// Number of live entries in `fields`.
    pub n_fields: usize,
    /// Total input-report extent in bits (fields and padding).
    pub report_len_bits: u16,
}

/// Parse/decode failures.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum HidError {
    /// Descriptor truncated mid-item.
    Short,
    /// Item tag outside the supported subset (the item's first byte).
    UnknownItem(u8),
    /// More than `MAX_FIELDS` decoded fields.
    TooManyFields,
    /// No Generic Desktop axis in the descriptor (js-criterion).
    NotStick,
    /// Report ID declared: unsupported (ruling 4 — T.16000M uses none;
    /// support deferred).
    ReportId,
}

/// js-criterion: the table feeds at least one axis
/// (X/Y/Z/Rx/Ry/Rz/Slider/Hat).
pub fn has_axes(t: &HidTable) -> bool {
    t.fields
        .iter()
        .take(t.n_fields)
        .any(|f| matches!(f.kind, FieldKind::Axis(_)))
}

//! HID parser state: the item-state table of HID 1.11 §5.4.
//!
//! Globals persist across items; locals reset after each Main item.
//! The state classifies Input controls by page+usage and appends
//! fields to the decode table: axis ids by first appearance (ruling
//! 3), a hat switch contributing two axes (ruling 2), signedness from
//! the logical minimum (ruling 5, §5.8).

use super::{
    FieldKind, GD_RX, GD_RY, GD_RZ, GD_SLIDER, GD_X, GD_Y, GD_Z, HatPart, HidError, HidTable,
    MAX_FIELDS, PAGE_BUTTON, PAGE_GENERIC_DESKTOP, ReportField,
};

/// Pending local usages per Main item: unique-usages-per-control
/// descriptors use a Usage Minimum/Maximum range, not long lists
/// (§6.2.2.8); more than this many on one item is rejected.
const MAX_USAGES: usize = 8;
/// Axis-id map capacity: one slot per named Generic Desktop axis usage.
const MAX_AXIS_IDS: usize = 8;

/// Parser state: globals persist across items, locals reset after each
/// Main item (HID 1.11 §5.4).
pub(super) struct State {
    pub(super) page: u16,
    pub(super) logical_min: i32,
    pub(super) bits: u8,
    pub(super) count: u8,
    usages: [u16; MAX_USAGES],
    n_usages: usize,
    usage_range: bool,
    usage_min: u16,
    usage_max: u16,
    /// Input-report bit offset; Output/Feature reports keep their own.
    pub(super) offset: u32,
    next_axis: u8,
    axis_ids: [(u16, u8); MAX_AXIS_IDS],
    n_axis_ids: usize,
    n_axes: usize,
}

impl State {
    pub(super) fn new() -> Self {
        State {
            page: 0,
            logical_min: 0,
            bits: 0,
            count: 0,
            usages: [0; MAX_USAGES],
            n_usages: 0,
            usage_range: false,
            usage_min: 0,
            usage_max: 0,
            offset: 0,
            next_axis: 0,
            axis_ids: [(0, 0); MAX_AXIS_IDS],
            n_axis_ids: 0,
            n_axes: 0,
        }
    }

    /// Total input-report extent seen so far (fields and padding).
    pub(super) fn axes_seen(&self) -> usize {
        self.n_axes
    }

    /// Input-report bit offset accumulated so far.
    pub(super) fn offset(&self) -> u32 {
        self.offset
    }

    /// Local Usage (unsigned, §6.2.2.8); extended usages carry the
    /// page in the high half, ignored here. Queue overflow — more
    /// than `MAX_USAGES` usages queued on one Main item — is a loud
    /// parse error: silently reassigning the tail usage would mis-map
    /// later controls onto a duplicated field.
    pub(super) fn add_usage(&mut self, val: i32) -> Result<(), HidError> {
        let u = ((val as u32) & 0xFFFF) as u16;
        if self.n_usages >= MAX_USAGES {
            return Err(HidError::TooManyFields);
        }
        self.usages[self.n_usages] = u;
        self.n_usages += 1;
        Ok(())
    }

    /// Local Usage Minimum: starts a sequential usage range (§6.2.2.8).
    pub(super) fn set_usage_min(&mut self, val: i32) {
        self.usage_min = ((val as u32) & 0xFFFF) as u16;
        self.usage_range = true;
    }

    /// Local Usage Maximum: ends the range.
    pub(super) fn set_usage_max(&mut self, val: i32) {
        self.usage_max = ((val as u32) & 0xFFFF) as u16;
    }

    /// Drop local items after a Main item (§5.4).
    pub(super) fn clear_locals(&mut self) {
        self.n_usages = 0;
        self.usage_range = false;
        self.usage_min = 0;
        self.usage_max = 0;
    }

    /// Usage for control `ctrl` of the pending Main item: the
    /// sequential queue first, then a Usage Min/Max range, then the
    /// repeat rule; `None` marks usage-less padding (§6.2.2.9).
    fn usage_at(&self, ctrl: u8) -> Option<u16> {
        let i = usize::from(ctrl);
        if i < self.n_usages {
            return Some(self.usages[i]);
        }
        if self.usage_range {
            // Sequential range assignment, clamped at Usage Maximum.
            return Some(
                self.usage_min
                    .saturating_add(u16::from(ctrl))
                    .min(self.usage_max),
            );
        }
        if self.n_usages > 0 {
            return Some(self.usages[self.n_usages - 1]);
        }
        None
    }

    /// One Input Main item: `count` controls of `bits` bits each,
    /// classified by page+usage; other usages and padding still
    /// consume report bits (§6.2.2.9).
    pub(super) fn do_input(&mut self, table: &mut HidTable) -> Result<(), HidError> {
        for ctrl in 0..self.count {
            let off = self.offset;
            if let Some(usage) = self.usage_at(ctrl) {
                if self.page == PAGE_GENERIC_DESKTOP {
                    self.gd_fields(usage, off, table)?;
                } else if self.page == PAGE_BUTTON {
                    self.button_field(usage, off, table)?;
                }
            }
            self.offset = off
                .checked_add(u32::from(self.bits))
                .ok_or(HidError::TooManyFields)?;
            if self.offset > u16::MAX as u32 {
                // The u16 extent caps the report; a pathological
                // size*count product maps to the capacity error.
                return Err(HidError::TooManyFields);
            }
        }
        Ok(())
    }

    /// Generic Desktop controls: named axis usages take ids by first
    /// appearance; a hat takes the NEXT TWO ids at its position
    /// (ruling 2/3).
    fn gd_fields(&mut self, usage: u16, off: u32, table: &mut HidTable) -> Result<(), HidError> {
        if usage == super::GD_HAT {
            let hx = self.axis_id(super::GD_HAT)?;
            let hy = self.fresh_axis()?; // second hat: fresh y, reused x
            self.push(table, FieldKind::Axis(hx), HatPart::HatX, off)?;
            self.push(table, FieldKind::Axis(hy), HatPart::HatY, off)?;
            return Ok(());
        }
        if is_axis_usage(usage) {
            let id = self.axis_id(usage)?;
            self.push(table, FieldKind::Axis(id), HatPart::NotHat, off)?;
        }
        // Other GD usages (Pointer, Joystick, ...): bits without a field.
        Ok(())
    }

    /// Button-page usage N → button id N-1 (ruling 3); usage 0 or
    /// beyond the u8 id space consumes bits without a field.
    fn button_field(&mut self, usage: u16, off: u32, table: &mut HidTable) -> Result<(), HidError> {
        if let Some(id) = usage.checked_sub(1).and_then(|v| u8::try_from(v).ok()) {
            self.push(table, FieldKind::Button(id), HatPart::NotHat, off)?;
        }
        Ok(())
    }

    /// Axis id by first appearance: reuse the recorded id, else a fresh
    /// one (ruling 3). Capacity fits the 8 named axis usages.
    fn axis_id(&mut self, usage: u16) -> Result<u8, HidError> {
        if let Some(&(_, id)) = self
            .axis_ids
            .iter()
            .take(self.n_axis_ids)
            .find(|(u, _)| *u == usage)
        {
            return Ok(id);
        }
        if self.n_axis_ids >= MAX_AXIS_IDS {
            return Err(HidError::TooManyFields);
        }
        let id = self.fresh_axis()?;
        self.axis_ids[self.n_axis_ids] = (usage, id);
        self.n_axis_ids += 1;
        Ok(id)
    }

    /// Next unused axis id; MAX_FIELDS bounds the count, the checked
    /// add keeps that explicit.
    fn fresh_axis(&mut self) -> Result<u8, HidError> {
        let id = self.next_axis;
        self.next_axis = self
            .next_axis
            .checked_add(1)
            .ok_or(HidError::TooManyFields)?;
        Ok(id)
    }

    /// Append one field at bit offset `off` with the current width and
    /// signedness (ruling 5, §5.8).
    fn push(
        &mut self,
        table: &mut HidTable,
        kind: FieldKind,
        hat: HatPart,
        off: u32,
    ) -> Result<(), HidError> {
        if table.n_fields >= MAX_FIELDS {
            return Err(HidError::TooManyFields);
        }
        let offset_bits = u16::try_from(off).map_err(|_| HidError::TooManyFields)?;
        table.fields[table.n_fields] = ReportField {
            kind,
            bits: self.bits,
            offset_bits,
            signed: self.logical_min < 0,
            hat,
        };
        table.n_fields += 1;
        if matches!(kind, FieldKind::Axis(_)) {
            self.n_axes += 1;
        }
        Ok(())
    }
}

/// Named Generic Desktop axis usages (HID Usage Tables §4; hat separate).
fn is_axis_usage(usage: u16) -> bool {
    matches!(
        usage,
        GD_X | GD_Y | GD_Z | GD_RX | GD_RY | GD_RZ | GD_SLIDER
    )
}

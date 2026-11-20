//! The pure session core: per-slot baseline state and the report →
//! event diff. No MMIO, no statics, no allocation — the host tests
//! drive this directly with scripted reports.

use super::HidEvent;
use crate::drivers::usb::hid::{EMPTY_FIELD, FieldKind, HidTable, MAX_FIELDS, decode_report};

/// Baseline axis capacity: axis ids are first-appearance numbers and
/// real sticks stay far below 16 (the fixture's 7 includes the two
/// hat components).
pub(crate) const N_AXES: usize = 16;

/// Baseline button bitmap bits: sticks top out at 16-32 buttons.
pub(crate) const N_BUTTONS: u32 = 32;

/// Inactive-slot table template.
const EMPTY_TABLE: HidTable = HidTable {
    fields: [EMPTY_FIELD; MAX_FIELDS],
    n_fields: 0,
    report_len_bits: 0,
};

/// Per-slot HID session (ruling 6: static, bounded, under
/// `USB_HID_LOCK`): the parsed decode table plus the baseline the
/// next report is diffed against. `init` marks that no report has been
/// accepted yet.
pub(crate) struct SlotSession {
    pub(crate) attached: bool,
    table: HidTable,
    init: bool,
    /// Baseline axis values, indexed by axis id.
    axes: [i16; N_AXES],
    /// Baseline button bitmap: bit b = button b held down.
    buttons: u32,
}

/// Open-time baseline snapshot for the devfs /dev/jsN init burst (task
/// 9): the session's current state plus the per-control record counts
/// the burst emits.
pub struct Baseline {
    /// Axis values by id; entries `0..axis_n` are meaningful.
    pub axes: [i16; N_AXES],
    /// Axis init-record count (clamped to the baseline width).
    pub axis_n: usize,
    /// Button bitmap: bit b = button b held down.
    pub buttons: u32,
    /// Button init-record count (clamped to the bitmap width).
    pub button_n: usize,
}

impl SlotSession {
    /// Snapshot for the devfs /dev/jsN open-time init burst (task 9):
    /// the current baseline plus the per-control record counts (the
    /// highest axis/button id in the table + 1, clamped to the same
    /// widths `apply_report` enforces — pathological descriptors must
    /// not push the burst past the baseline arrays). `None` while the
    /// slot holds no attached session.
    pub fn snapshot(&self) -> Option<Baseline> {
        if !self.attached {
            return None;
        }
        let mut axis_n = 0usize;
        let mut button_n = 0usize;
        for f in self.table.fields.iter().take(self.table.n_fields) {
            match f.kind {
                FieldKind::Axis(a) => axis_n = axis_n.max(usize::from(a) + 1),
                FieldKind::Button(b) => button_n = button_n.max(usize::from(b) + 1),
            }
        }
        Some(Baseline {
            axes: self.axes,
            axis_n: axis_n.min(N_AXES),
            buttons: self.buttons,
            button_n: button_n.min(N_BUTTONS as usize),
        })
    }

    pub(crate) const fn empty() -> Self {
        SlotSession {
            attached: false,
            table: EMPTY_TABLE,
            init: false,
            axes: [0; N_AXES],
            buttons: 0,
        }
    }

    /// Start a session over `table` with a fresh baseline: the next
    /// accepted report is the init burst.
    pub(crate) fn from_table(table: HidTable) -> Self {
        SlotSession {
            attached: true,
            table,
            init: false,
            axes: [0; N_AXES],
            buttons: 0,
        }
    }
}

/// The testable core (ruling 4): decode `report` through the
/// session's table and diff it against the baseline, filling `out`
/// with `(event, is_init)` pairs; returns how many.
///
/// - First accepted report: every decoded field is emitted with
///   `is_init = true` (the komnata `JS_EVENT_INIT` drain analog) and
///   becomes the baseline — buttons included, so consumers learn the
///   released state too.
/// - Later reports: an axis emits only when it moved; a button only
///   on an edge (down/up transition).
/// - Decode count 0 (report shorter than the table's byte extent):
///   dropped silently — a corrupt endpoint must not spin or spam the
///   event stream — and the baseline stays untouched.
/// - Field ids beyond the baseline widths are dropped: the parser
///   keeps ids well below both caps; this is defense for pathological
///   descriptors, not an expected path.
pub(crate) fn apply_report(
    s: &mut SlotSession,
    report: &[u8],
    out: &mut [(HidEvent, bool)],
) -> usize {
    let mut scratch = [(FieldKind::Axis(0), 0i16); MAX_FIELDS];
    let n = decode_report(&s.table, report, &mut scratch);
    if n == 0 {
        // Decode sentinel: the report is shorter than the table's byte
        // extent — dropped silently, baseline untouched, so a corrupt
        // endpoint can neither spin the poll loop nor spam the event
        // stream.
        return 0;
    }
    // First accepted report: every field is an init event.
    let init = !s.init;
    let mut m = 0usize;
    for (kind, value) in scratch.iter().take(n) {
        let ev = match *kind {
            FieldKind::Axis(a) => {
                let idx = usize::from(a);
                if idx >= N_AXES {
                    continue; // pathological descriptor; ids stay ≪ 16
                }
                if !init && s.axes[idx] == *value {
                    continue; // unmoved axis: no event
                }
                s.axes[idx] = *value;
                HidEvent::Axis {
                    axis: a,
                    value: *value,
                }
            }
            FieldKind::Button(b) => {
                if u32::from(b) >= N_BUTTONS {
                    continue; // pathological descriptor; ids stay ≪ 32
                }
                // b < N_BUTTONS checked above, so the shift stays in range.
                let bit = 1u32 << b;
                let down = *value != 0;
                if !init && (s.buttons & bit != 0) == down {
                    continue; // no edge: held state unchanged
                }
                if down {
                    s.buttons |= bit;
                } else {
                    s.buttons &= !bit;
                }
                HidEvent::Button { index: b, down }
            }
        };
        // `m` can only reach `n ≤ MAX_FIELDS`; get_mut keeps the match
        // panic-free for any caller slice.
        if let Some(slot) = out.get_mut(m) {
            *slot = (ev, init);
            m += 1;
        }
    }
    s.init = true;
    m
}

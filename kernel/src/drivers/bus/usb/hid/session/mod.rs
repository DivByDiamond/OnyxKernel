//! HID session: the attach/poll pair that turns an enumerated HID slot
//! into a diff-based event stream.
//!
//! [`attach`] runs on the boot probe: fetch the interface's report
//! descriptor over the control bus (HID 1.11 §7.2.1), parse it into a
//! decode table, arm the slot's interrupt-IN pipe through the owner
//! dispatch, and install the per-slot session. A descriptor without
//! axes is NotStick — the slot stays a plain enumerated device (the
//! usb-kbd smoke case).
//!
//! [`poll`] runs in the 100 Hz input loop: one NON-BLOCKING
//! interrupt-pipe read per attached slot (ruling 2: both controllers
//! idle at `Ok(0)`), decode, and diff against the session baseline in
//! [`diff::apply_report`] — the pure, host-tested core. The first
//! accepted report emits every field with the init flag (the
//! komnata-session `JS_EVENT_INIT` drain analog); later reports emit
//! only moved axes and button edges.
//!
//! Lock order: `USB_HID_LOCK` → `USB_DEV_LOCK` (the poll's transfer
//! takes the registry lock internally) — never the reverse.
//!
//! Layout: the session lives in a `hid/session/` subfolder because
//! `hid/` was already at the ≤4-code-files-per-folder cap
//! (`items.rs`, `mod.rs`, `report.rs`, `state.rs`) — the same shape
//! as `report.rs` + `report/`: code file with a dedicated subfolder,
//! tests next to their modules.

// `&raw mut` on a `static mut` is deliberate: clippy's suggested bare
// `HID_SESSIONS` would create a reference to it and trip
// `static_mut_refs` (the same allowance virtio_input carries).
#![allow(clippy::deref_addrof)]

use onyx_core::errno::{Errno, KResult};

use crate::drivers::usb::dev::requests::ControlBus;
use crate::drivers::usb::dev::{MAX_SLOTS, Owner, slot as registry_slot};

mod diff;
mod fetch;
#[cfg(test)]
mod fetch_tests;
#[cfg(test)]
mod tests;

pub use diff::Baseline;
pub(crate) use diff::{SlotSession, apply_report};
pub(crate) use fetch::fetch_table;

/// Interrupt-IN report buffer: full/low-speed interrupt endpoints cap
/// at 64 bytes (USB 2.0 §5.6.3) — the same range the arm paths
/// enforce, so one stack buffer serves every poll.
const MAX_REPORT: usize = 64;

/// Pending-event capacity: one report emits at most one event per
/// decoded field, and the parser caps fields at 64.
const MAX_PEND: usize = 64;

/// One decoded HID input event of the session's diff stream.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum HidEvent {
    /// An axis moved to `value` (axis id = first-appearance order in
    /// the descriptor).
    Axis { axis: u8, value: i16 },
    /// Button `index` (Button-page usage minus one) crossed an edge.
    Button { index: u8, down: bool },
}

/// Session store: per-slot sessions plus the one pending-event FIFO a
/// poll tick drains (poll decodes one report per call and hands its
/// events out one by one).
struct SessionStore {
    slots: [SlotSession; MAX_SLOTS],
    /// Queued `(event, is_init, slot)` triples, FIFO from `pend_head`.
    /// The producing session's slot rides every queued event so the
    /// /dev/jsN drain routes ring pushes by session, never by registry
    /// class (a HID-class slot without a session must not steal them).
    pend: [(HidEvent, bool, usize); MAX_PEND],
    pend_head: usize,
    pend_len: usize,
}

impl SessionStore {
    const fn new() -> Self {
        SessionStore {
            slots: [const { SlotSession::empty() }; MAX_SLOTS],
            pend: [(HidEvent::Axis { axis: 0, value: 0 }, false, 0); MAX_PEND],
            pend_head: 0,
            pend_len: 0,
        }
    }
}

// All session state is serialized on the HID lock, mirroring the
// virtio_input INPUTS pattern: attach writes it once on the
// single-threaded boot probe (SIE=0), poll runs in the serialized
// single-consumer input loop. Order: USB_HID_LOCK → USB_DEV_LOCK
// (inside usb::interrupt_transfer) — never the reverse.
static USB_HID_LOCK: crate::sync::SpinLock = crate::sync::SpinLock::new();
static mut HID_SESSIONS: SessionStore = SessionStore::new();

/// Attach a HID session to registry slot `slot_idx` (ruling 3): fetch
/// and parse the report descriptor over `bus`, arm the slot's
/// interrupt-IN pipe through the owner dispatch, then install the
/// per-slot session. On `Err` the slot stays a plain enumerated
/// device — a NotStick keyboard is the expected case.
///
/// Error contract: `Errno::NoEnt` — not a stick (or an inactive slot
/// id); `Errno::NoSys` — the owner has no interrupt path (EHCI,
/// ruling 1); `Errno::Inval` — malformed descriptor; anything else —
/// the bus or the arm failed.
///
/// # Safety
///
/// Boot-probe context only: single-threaded, SIE=0, `bus`'s owner
/// controller initialized and owning the slot's device address. The
/// registry read inside takes `USB_DEV_LOCK` (released before the
/// control transfers, which MMIO-wait) — hold neither lock across the
/// call.
pub unsafe fn attach(slot_idx: usize, bus: &mut dyn ControlBus) -> KResult<()> {
    let s = registry_slot(slot_idx).ok_or(Errno::NoEnt)?;
    let table = fetch_table(bus, s.iface)?;
    // SAFETY: boot-probe context per the contract; the registry lock
    // is free here (dev::slot released it before the control transfers).
    unsafe {
        match s.owner {
            Owner::Xhci => crate::drivers::usb::xhci::xhci_arm_interrupt(slot_idx)?,
            Owner::Ohci => crate::drivers::usb::ohci::ohci_arm_interrupt(slot_idx)?,
            // EHCI interrupt transfers are out of scope (ruling 1): a
            // high-speed interrupt device fails the arm with NoSys —
            // the boot probe kwrns it, nothing is silently ignored.
            Owner::Ehci => return Err(Errno::NoSys),
        }
    }
    USB_HID_LOCK.lock();
    // SAFETY: USB_HID_LOCK serializes the store; `slot_idx` was just
    // validated in range by registry_slot (MAX_SLOTS-bounded).
    unsafe {
        let store = &mut *(&raw mut HID_SESSIONS);
        store.slots[slot_idx] = SlotSession::from_table(table);
    }
    USB_HID_LOCK.unlock();
    Ok(())
}

/// One poll tick: pop the next queued event, else run one
/// NON-BLOCKING interrupt transfer per attached slot (ruling 2:
/// `Ok(0)` is idle on both controllers), decode a report through the
/// slot's stored table, and queue the diff events. Returns one
/// `(event, is_init, slot)` triple — the slot is the attached
/// session's registry slot, so the /dev/jsN ring push lands in the
/// session's ring even when other HID-class devices hold registry
/// slots — `None` when every attached slot is idle. No allocation
/// happens here — the scratch and queue buffers are static.
pub fn poll() -> Option<(HidEvent, bool, usize)> {
    USB_HID_LOCK.lock();
    // SAFETY: USB_HID_LOCK serializes the session store; the transfer
    // inside runs on the serialized single-consumer USB poll context
    // (SIE=0 per `crate::sync`, the contract
    // `usb::interrupt_transfer` states) and takes USB_DEV_LOCK
    // internally — order USB_HID_LOCK → USB_DEV_LOCK, never reversed.
    let ev = unsafe { poll_locked(&mut *(&raw mut HID_SESSIONS)) };
    USB_HID_LOCK.unlock();
    ev
}

/// Locked poll body (see [`poll`]).
///
/// # Safety
///
/// Caller holds `USB_HID_LOCK`; the USB transfer paths it calls run
/// on the serialized single-consumer poll context (SIE=0).
unsafe fn poll_locked(store: &mut SessionStore) -> Option<(HidEvent, bool, usize)> {
    unsafe {
        if let Some(ev) = pop_pend(store) {
            return Some(ev);
        }
        for idx in 0..MAX_SLOTS {
            if !store.slots[idx].attached {
                continue;
            }
            let mut buf = [0u8; MAX_REPORT];
            // One NON-BLOCKING attempt on this slot's armed pipe.
            match crate::drivers::usb::interrupt_transfer(idx, &mut buf) {
                // Idle: the armed pipe holds no report yet.
                Ok(0) => {}
                Ok(n) => {
                    let end = (n as usize).min(buf.len());
                    let mut evs = [(HidEvent::Axis { axis: 0, value: 0 }, false); MAX_PEND];
                    let m = apply_report(&mut store.slots[idx], &buf[..end], &mut evs);
                    push_pend(store, idx, &evs[..m]);
                    return pop_pend(store);
                }
                // Error-retired: the controller path re-arms itself,
                // so a transient stall must not kill the session —
                // keep polling.
                Err(_) => {}
            }
        }
        None
    }
}

/// Pop the oldest queued event.
fn pop_pend(store: &mut SessionStore) -> Option<(HidEvent, bool, usize)> {
    if store.pend_len == 0 {
        return None;
    }
    let ev = store.pend[store.pend_head];
    store.pend_head = (store.pend_head + 1) % MAX_PEND;
    store.pend_len -= 1;
    Some(ev)
}

/// Queue `slot`'s `evs` FIFO; a full queue drops the tail — capacity
/// equals the max events one report can emit, and the 100 Hz consumer
/// drains long before an overflow could only be caused by a stuck one.
fn push_pend(store: &mut SessionStore, slot: usize, evs: &[(HidEvent, bool)]) {
    for (ev, init) in evs {
        if store.pend_len >= MAX_PEND {
            break;
        }
        store.pend[(store.pend_head + store.pend_len) % MAX_PEND] = (*ev, *init, slot);
        store.pend_len += 1;
    }
}

/// Registry slots holding an attached stick session — the /dev/jsN
/// namespace backing (task 9): no session, no dead node.
pub fn stick_slots() -> [bool; MAX_SLOTS] {
    let mut out = [false; MAX_SLOTS];
    USB_HID_LOCK.lock();
    // SAFETY: USB_HID_LOCK serializes the store; plain copy-out.
    unsafe {
        let store = &*(&raw const HID_SESSIONS);
        for (i, live) in out.iter_mut().enumerate() {
            *live = store.slots[i].attached;
        }
    }
    USB_HID_LOCK.unlock();
    out
}

/// Baseline of the attached session on slot `slot_idx` — the /dev/jsN
/// open-time init-burst source (task 9); `None` when out of range or
/// no session is attached.
pub fn baseline(slot_idx: usize) -> Option<Baseline> {
    if slot_idx >= MAX_SLOTS {
        return None;
    }
    USB_HID_LOCK.lock();
    // SAFETY: USB_HID_LOCK serializes the store; slot_idx < MAX_SLOTS.
    let b = unsafe { (&*(&raw const HID_SESSIONS)).slots[slot_idx].snapshot() };
    USB_HID_LOCK.unlock();
    b
}

//! Per-device xHCI state: one [`XhciDev`] per granted controller slot.
//!
//! Everything that used to live in single-device globals (`G_XHCI.slot`,
//! the transfer-ring array, the armed interrupt channel and its parked
//! completions) is owned per slot here, indexed by the controller's slot
//! id (`DEVS[slot_id - 1]`). The shared event ring is demuxed onto these
//! records by (slot id, endpoint id), so several HID devices can be
//! armed at once without sharing rings or events.
//!
//! Access is single-owner like `G_XHCI` (boot-probe, then the serialized
//! SIE=0 USB poll — `crate::sync`); the routing/claim/release logic is
//! written over plain slices so the host tests exercise it directly.

use core::ptr;
use onyx_core::errno::{Errno, KResult};

use super::pages::PageSet;
use super::ring::{Trb, TrbRing};

#[cfg(test)]
mod tests;

/// Controller slots tracked (ids 1..=MAX_DEVS). A controller that
/// grants a higher id gets the slot disabled again by the bring-up.
pub(crate) const MAX_DEVS: usize = 8;
/// Pre-posted interrupt buffers per armed endpoint, and the capacity
/// of its parked-completion FIFO (one park per in-flight buffer).
pub(crate) const N_BUFS: usize = 8;
/// Endpoint contexts per device (DCI 1..=31 → ring index DCI-1).
pub(crate) const N_RINGS: usize = 32;

/// Transfer-event slot id: DW3 bits [31:24] (§6.4.2.1).
pub(crate) fn event_slot(ev: &Trb) -> u8 {
    (ev.params[3] >> 24) as u8
}

/// Transfer-event endpoint id (DCI): DW3 bits [20:16] (§6.4.2.1).
pub(crate) fn event_dci(ev: &Trb) -> u8 {
    ((ev.params[3] >> 16) & 0x1F) as u8
}

/// Armed interrupt-IN channel of one device plus the completions that
/// landed while another transfer owned the event ring.
pub(crate) struct IntrChan {
    pub(crate) armed: bool,
    /// Doorbell/context index of the armed endpoint.
    pub(crate) dci: u8,
    pub(crate) max_pkt: u16,
    /// Interrupt ring base — bounds-check for completion TRB pointers.
    pub(crate) ring_pa: u64,
    /// Pre-posted buffer pool page (N_BUFS * max_pkt bytes).
    pub(crate) bufs_pa: u64,
    parked: [Trb; N_BUFS],
    park_head: usize,
    park_len: usize,
}

impl IntrChan {
    const fn new() -> Self {
        IntrChan {
            armed: false,
            dci: 0,
            max_pkt: 0,
            ring_pa: 0,
            bufs_pa: 0,
            parked: [Trb::zero(); N_BUFS],
            park_head: 0,
            park_len: 0,
        }
    }

    /// Park `ev` when it completes this channel's endpoint. A full FIFO
    /// drops it (cannot happen: one park per in-flight buffer).
    pub(crate) fn park(&mut self, ev: &Trb) -> bool {
        if !self.armed || event_dci(ev) != self.dci || self.park_len >= N_BUFS {
            return false;
        }
        self.parked[(self.park_head + self.park_len) % N_BUFS] = *ev;
        self.park_len += 1;
        true
    }

    /// Pop the oldest parked completion.
    pub(crate) fn take_parked(&mut self) -> Option<Trb> {
        if self.park_len == 0 {
            return None;
        }
        let ev = self.parked[self.park_head];
        self.park_head = (self.park_head + 1) % N_BUFS;
        self.park_len -= 1;
        Some(ev)
    }
}

pub(crate) const EMPTY_RING: TrbRing = TrbRing {
    base: ptr::null_mut(),
    phys: 0,
    size: 0,
    enqueue: 0,
    cycle: false,
};

/// One addressed device: its root port and speed, its transfer rings
/// (a null `base` marks an unpublished endpoint), the pages it owns
/// and its interrupt channel.
pub(crate) struct XhciDev {
    pub(crate) in_use: bool,
    pub(crate) port: u8,
    pub(crate) speed: u8,
    pub(crate) rings: [TrbRing; N_RINGS],
    pub(crate) pages: PageSet,
    pub(crate) intr: IntrChan,
}

impl XhciDev {
    pub(crate) const fn new() -> Self {
        XhciDev {
            in_use: false,
            port: 0,
            speed: 0,
            rings: [EMPTY_RING; N_RINGS],
            pages: PageSet::new(),
            intr: IntrChan::new(),
        }
    }

    /// Ring of endpoint `dci` when published.
    pub(crate) fn ring_mut(&mut self, dci: u8) -> Option<&mut TrbRing> {
        let r = self.rings.get_mut((dci as usize).checked_sub(1)?)?;
        if r.base.is_null() { None } else { Some(r) }
    }

    /// Free every owned page through `free` and return to the empty state.
    pub(crate) fn release(&mut self, free: impl FnMut(u64)) {
        self.pages.release_all(free);
        *self = XhciDev::new();
    }
}

/// Record controller slot `slot_id` as a device on root port `port`.
/// `Range` for id 0 or beyond the table, `Busy` when the slot is
/// already live or another device already sits on `port`.
pub(crate) fn claim(
    devs: &mut [XhciDev],
    slot_id: u8,
    port: u8,
    speed: u8,
) -> KResult<&mut XhciDev> {
    let idx = (slot_id as usize).checked_sub(1).ok_or(Errno::Range)?;
    if idx >= devs.len() {
        return Err(Errno::Range);
    }
    if devs.iter().any(|d| d.in_use && d.port == port) || devs[idx].in_use {
        return Err(Errno::Busy);
    }
    let d = &mut devs[idx];
    d.in_use = true;
    d.port = port;
    d.speed = speed;
    Ok(d)
}

/// Route a transfer event to the owning device's parked FIFO when it
/// completes that device's armed endpoint. Events of unknown slots,
/// dead devices or unarmed endpoints are not parked (`false`).
pub(crate) fn route_park(devs: &mut [XhciDev], ev: &Trb) -> bool {
    let Some(idx) = (event_slot(ev) as usize).checked_sub(1) else {
        return false;
    };
    match devs.get_mut(idx) {
        Some(d) if d.in_use => d.intr.park(ev),
        _ => false,
    }
}

static mut DEVS: [XhciDev; MAX_DEVS] = [const { XhciDev::new() }; MAX_DEVS];

/// The whole device table.
///
/// # Safety
///
/// Single-owner driver path only (boot probe, or the serialized SIE=0
/// USB poll): the returned slice must not outlive the call chain or
/// coexist with another reference to `DEVS`.
pub(crate) unsafe fn table() -> &'static mut [XhciDev] {
    let p = &raw mut DEVS;
    // SAFETY: exclusive access per the contract above.
    unsafe { &mut *p }
}

/// Live device of controller slot `slot_id`, if any.
///
/// # Safety
///
/// Same contract as [`table`].
pub(crate) unsafe fn dev(slot_id: u8) -> Option<&'static mut XhciDev> {
    // SAFETY: forwarded contract.
    let t = unsafe { table() };
    t.get_mut((slot_id as usize).checked_sub(1)?)
        .filter(|d| d.in_use)
}

/// Park `ev` for the interrupt poll of the device that owns it.
///
/// # Safety
///
/// Single-owner event-ring path (SIE=0, USB transfers serialized).
pub(super) unsafe fn park_if_armed(ev: &Trb) -> bool {
    // SAFETY: forwarded contract.
    route_park(unsafe { table() }, ev)
}

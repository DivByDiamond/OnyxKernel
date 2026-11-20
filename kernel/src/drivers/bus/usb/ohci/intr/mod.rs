//! OHCI periodic interrupt transfers: the channel a full-speed HID
//! device — the SG2000 joystick behind the OHCI companion — delivers
//! reports on.
//!
//! Periodic schedule: the HCCA `init_ohci` allocated (32 interrupt-list
//! pointers at its start, one serviced per 1-ms frame at
//! `frame_number & 31`) with PLE enabled since init — harmless over
//! the all-zero table until an ED is linked. `arm` (arm.rs) builds the
//! slot's interrupt-IN ED/TD from the pure builders below and links
//! the ED into the lists at the descriptor's interval.
//! `ohci_poll_interrupt` is NON-BLOCKING by design (ruling 4): one
//! pool read decides between idle and a retired report — no MMIO
//! anywhere in the poll path.
//!
//! Hardware validation on a live Milk-V Duo S is deferred until the
//! board is in hand: the SG2000 MMIO window does not exist under QEMU,
//! so the proof here is host tests of the pure builders plus a full
//! build.

use core::ptr;
use onyx_core::errno::{Errno, KResult};

use super::pool::{ohci_ed_ptr, ohci_td_phys, ohci_td_ptr};
use super::{
    ED_EN_SHIFT, ED_FA_SHIFT, ED_HEAD_CARRY, ED_MPS_SHIFT, ED_SPEED_FULL, TD_CC_MASK,
    TD_CC_NOERROR, TD_CC_NOT_ACCESSED, TD_DI_NO_INTR, TD_DP_IN, TD_R_ROUND, TD_T_CARRY,
};

mod arm;
#[cfg(test)]
mod tests;

pub use arm::ohci_arm_interrupt;

/// HCCA interrupt lists per spec 1.0a §4.4.1 — one pointer per 1-ms
/// frame slot; the start of the 256-byte HCCA page.
const HCCA_LISTS: usize = 32;

/// ED control word for a full-speed interrupt-IN endpoint (spec 1.0a
/// §4.2.2 field table; cf. Linux ohci.h, QEMU hcd-ohci.c): FA bits 0-6,
/// EN bits 7-10, S = 0 (full speed — the registry carries no speed bit
/// and the SG2000 companion path targets full-speed HID), K = 0, F = 0
/// (general TDs, not isochronous), MPS bits 16-26.
pub(super) fn intr_ed_fields(addr: u8, endpoint: u8, mps: u16) -> u32 {
    u32::from(addr & 0x7F) << ED_FA_SHIFT
        | u32::from(endpoint & 0x0F) << ED_EN_SHIFT
        | ED_SPEED_FULL
        | u32::from(mps.min(0x3FF)) << ED_MPS_SHIFT
}

/// Control word of a fresh interrupt-IN report TD (spec 1.0a §4.3.1):
/// DP = 10b IN, CC = NOT_ACCESSED (the HCD marker the HC overwrites on
/// retirement), R set (a short report is fine), DI = 7 no interrupt
/// (the poll never arms controller IRQs), T = 00b — the data toggle
/// rides the ED carry the HC maintains across TDs.
pub(super) fn intr_td_fields() -> u32 {
    TD_CC_NOT_ACCESSED | TD_T_CARRY | TD_DI_NO_INTR | TD_DP_IN | TD_R_ROUND
}

/// HCCA list placement for a full/low-speed interrupt `bInterval` (ms):
/// list `frame_number & 31` is serviced each frame, so an ED linked
/// into lists `stride` frames apart is polled every `stride` ms.
/// The interval clamps to 1..=32 — `bInterval` 0 is an invalid
/// descriptor and degrades to 1 ms, and > 32 ms cannot be honored any
/// finer than the 32-list wheel.
pub(super) fn intr_list_indices(interval_ms: u8) -> (usize, usize) {
    let stride = usize::from(interval_ms).clamp(1, HCCA_LISTS);
    (0, stride)
}

/// Armed interrupt state — one armed HID slot (the registry's
/// single-device enumeration, mirroring the xHCI single-slot design).
struct IntrState {
    armed: bool,
    /// Registry slot the channel was armed for; arming a different
    /// slot while one is live reports `Busy` — one channel at a time.
    slot_idx: usize,
    ed_idx: usize,
    td_idx: usize,
    tail_td_idx: usize,
    /// Identity-mapped PMM page the HC DMAs reports into.
    buf_pa: u32,
    max_pkt: u16,
}

// Single-owner state, like the xHCI G_INTR: written once at arm on the
// single-threaded boot probe, then read/updated on the serialized poll
// path (SIE=0 per crate::sync).
static mut G_OHCI_INTR: IntrState = IntrState {
    armed: false,
    slot_idx: 0,
    ed_idx: 0,
    td_idx: 0,
    tail_td_idx: 0,
    buf_pa: 0,
    max_pkt: 0,
};

/// Poll the armed interrupt-IN channel once — NON-BLOCKING by design
/// (ruling 4): a single pool read decides; there is no MMIO and no
/// wait anywhere in this path.
///
/// Contract:
/// - `Ok(0)` — armed, nothing to read: the report TD is still queued
///   (the device NAKs until it has a report). Returns immediately; a
///   future input-poll loop may spin on this cheaply.
/// - `Ok(n > 0)` — one report of `n` bytes copied into `buf`; the TD
///   is re-armed for the next report before returning.
/// - `Err(Errno::Io)` — the TD retired with an error condition code
///   (stall, device-not-responding, ...). The TD is re-armed — the
///   head write clears the HC's halt bit — so polling may recover;
///   the caller decides whether to keep going.
/// - `Err(Errno::NoEnt)` — the channel was never armed;
///   `Err(Errno::Inval)` — `buf` is smaller than the endpoint's max
///   packet.
///
/// # Safety
///
/// Single-consumer poll context only (SIE=0 per `crate::sync`,
/// serialized with every other OHCI transfer path); requires a prior
/// successful arm. The registry lock is NOT held here — the
/// dispatcher's slot read took and released it already.
pub(crate) unsafe fn ohci_poll_interrupt(buf: &mut [u8]) -> KResult<u32> {
    // SAFETY: single-owner static state per the contract; every raw
    // pointer comes from pool indices recorded at arm, and the pool
    // reads below are volatile because the HC DMAs into them.
    unsafe {
        let g = &raw const G_OHCI_INTR;
        if !(*g).armed {
            return Err(Errno::NoEnt);
        }
        if buf.len() < (*g).max_pkt as usize {
            return Err(Errno::Inval);
        }
        let td = ohci_td_ptr((*g).td_idx);
        let ed = ohci_ed_ptr((*g).ed_idx);
        let cc = ptr::addr_of!((*td).control).read_volatile() & TD_CC_MASK;
        if cc == TD_CC_NOT_ACCESSED {
            // Still queued: idle — report nothing, at once.
            return Ok(0);
        }
        let head = ptr::addr_of!((*ed).head_td).read_volatile();
        let ok = cc == TD_CC_NOERROR;
        // A retired IN TD with cbp == 0 moved the whole buffer; a short
        // report leaves cbp one past its last byte (spec 1.0a §4.3.3
        // writeback; QEMU hcd-ohci.c applies the same rule). The buffer
        // never crosses a page, so the difference is the byte count.
        let n = if ok {
            let cbp = ptr::addr_of!((*td).cbp).read_volatile();
            if cbp == 0 {
                u32::from((*g).max_pkt)
            } else {
                cbp - (*g).buf_pa
            }
        } else {
            0
        };
        if ok && n > 0 {
            // SAFETY: buf_pa is the identity-mapped PMM page allocated
            // at arm; n ≤ max_pkt ≤ buf.len() (checked above).
            ptr::copy_nonoverlapping((*g).buf_pa as *const u8, buf.as_mut_ptr(), n as usize);
        }
        // Re-arm the same TD: fresh control word, buffers unchanged.
        // Writing head back to the report TD — preserving the HC's
        // data-toggle carry — re-queues it and, on an error retire,
        // clears the halt bit the HC set (the ED is live again).
        ptr::addr_of_mut!((*td).control).write_volatile(intr_td_fields());
        ptr::addr_of_mut!((*td).cbp).write_volatile((*g).buf_pa);
        ptr::addr_of_mut!((*td).next_td).write_volatile(ohci_td_phys((*g).tail_td_idx));
        ptr::addr_of_mut!((*td).be).write_volatile((*g).buf_pa + u32::from((*g).max_pkt) - 1);
        ptr::addr_of_mut!((*ed).head_td)
            .write_volatile(ohci_td_phys((*g).td_idx) | (head & ED_HEAD_CARRY));
        if ok { Ok(n) } else { Err(Errno::Io) }
    }
}

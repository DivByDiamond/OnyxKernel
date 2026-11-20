//! The arm half of the OHCI interrupt channel: registry lookup, ED/TD
//! build from the pure `intr` builders, HCCA list placement. The pure
//! builders' byte-exact host tests live in `../tests.rs`.

use onyx_core::errno::{Errno, KResult};
use onyx_core::fmt::Arg;

use super::super::pool::{
    ohci_alloc_ed, ohci_alloc_td, ohci_ed_phys, ohci_ed_ptr, ohci_release_ed, ohci_release_td,
    ohci_td_phys, ohci_td_ptr,
};
use super::super::{
    ED_TERMINATE, G_OHCI_HCCA_PA, G_OHCI_HCCA_READY, TD_CC_NOT_ACCESSED, TD_TERMINATE,
};
use super::{
    G_OHCI_INTR, HCCA_LISTS, IntrState, intr_ed_fields, intr_list_indices, intr_td_fields,
};
use crate::drivers::usb::dev::{Owner, slot as registry_slot};
use crate::mm::pmm;

/// Legal full/low-speed interrupt max-packet range (USB 2.0 §5.3.3:
/// interrupt endpoints 1..=64 bytes; the report buffer is one PMM
/// page, so a longer descriptor is rejected before it is armed).
const MAX_MPS: u16 = 64;

/// Arm the interrupt-IN endpoint of registry slot `slot_idx`: build the
/// ED/TD from the pure builders, allocate the report page, and link
/// the ED into the HCCA periodic lists at the descriptor's interval.
/// PLE has been on since `init_ohci` (harmless over the empty lists
/// until this link). The device's reports then land in the report page
/// once per interval, and [`super::ohci_poll_interrupt`] reaps them.
///
/// Idempotent: re-arming the same slot returns `Ok(())`; arming a
/// different slot while one is live reports `Busy` — one interrupt
/// channel at a time (mirroring the xHCI single-slot design).
///
/// # Safety
///
/// Requires a prior successful `init_ohci` (HCCA allocated and
/// programmed, controller operational); boot-probe context only —
/// single-threaded, SIE=0. The registry read takes `USB_DEV_LOCK`
/// internally, so the caller must not hold it. Error paths give back
/// every resource taken (ruling 3): the PMM page is freed and the
/// pool ED/TD indices are LIFO-released.
pub unsafe fn ohci_arm_interrupt(slot_idx: usize) -> KResult<()> {
    let s = registry_slot(slot_idx).ok_or(Errno::NoEnt)?;
    if s.owner != Owner::Ohci {
        return Err(Errno::NoSys); // the dispatcher never routes these here
    }
    if s.ep_in & 0x80 == 0 || s.max_packet == 0 || s.max_packet > MAX_MPS {
        return Err(Errno::Inval);
    }
    unsafe {
        let g = &raw const G_OHCI_INTR;
        if (*g).armed {
            if (*g).slot_idx == slot_idx {
                return Ok(()); // already armed
            }
            return Err(Errno::Busy); // one interrupt channel at a time
        }
        if !G_OHCI_HCCA_READY {
            return Err(Errno::NoSys); // init_ohci never completed
        }

        // Report buffer: one zeroed, identity-mapped PMM page.
        let buf_pa = pmm::alloc_zero()? as u32;
        // From here every failure unwinds exactly what was taken.
        let ed_idx = match ohci_alloc_ed() {
            Ok(i) => i,
            Err(e) => {
                // SAFETY: the PMM page was just allocated and is unused.
                pmm::free(u64::from(buf_pa));
                return Err(e);
            }
        };
        let td_idx = match ohci_alloc_td() {
            Ok(i) => i,
            Err(e) => {
                // SAFETY: LIFO release of the ED taken directly above.
                ohci_release_ed();
                // SAFETY: the PMM page is unused from here on.
                pmm::free(u64::from(buf_pa));
                return Err(e);
            }
        };
        let tail_td_idx = match ohci_alloc_td() {
            Ok(i) => i,
            Err(e) => {
                // SAFETY: LIFO releases in reverse allocation order.
                ohci_release_td();
                ohci_release_ed();
                // SAFETY: the PMM page is unused from here on.
                pmm::free(u64::from(buf_pa));
                return Err(e);
            }
        };

        // ED: the full-speed interrupt-IN endpoint of the slot.
        let ed = ohci_ed_ptr(ed_idx);
        (*ed).control = intr_ed_fields(s.addr, s.ep_in & 0x0F, s.max_packet);
        (*ed).tail_td = ohci_td_phys(tail_td_idx);
        (*ed).head_td = ohci_td_phys(td_idx);
        (*ed).next_ed = ED_TERMINATE;

        // Report TD, then the idle tail TD: the HC retires the report
        // and parks the ED (head == tail) until the poll re-arms it.
        let td = ohci_td_ptr(td_idx);
        (*td).control = intr_td_fields();
        (*td).cbp = buf_pa;
        (*td).next_td = ohci_td_phys(tail_td_idx);
        (*td).be = buf_pa + u32::from(s.max_packet) - 1;
        let tail = ohci_td_ptr(tail_td_idx);
        (*tail).control = TD_CC_NOT_ACCESSED;
        (*tail).cbp = 0;
        (*tail).next_td = TD_TERMINATE;
        (*tail).be = 0;

        // Link the ED into the HCCA periodic lists at the descriptor
        // interval — plain pointers: bit 0 (T) = 0 means linked. A
        // frame tick between the writes sees either 0 (empty list) or
        // the fully built ED, so no half-linked state is observable.
        let (_, stride) = intr_list_indices(s.interval);
        let hcca = G_OHCI_HCCA_PA as *mut u32;
        for i in (0..HCCA_LISTS).step_by(stride) {
            // SAFETY: hcca is the 256-byte identity-mapped PMM page
            // init_ohci allocated and programmed; i < HCCA_LISTS keeps
            // the write inside the interrupt table (offsets 0..127).
            hcca.add(i).write_volatile(ohci_ed_phys(ed_idx));
        }

        // SAFETY: single-owner boot path; the published state is fully
        // consistent (ED linked, TDs built, page owned).
        G_OHCI_INTR = IntrState {
            armed: true,
            slot_idx,
            ed_idx,
            td_idx,
            tail_td_idx,
            buf_pa,
            max_pkt: s.max_packet,
        };
        crate::kinf!(
            "ohci",
            "intr armed mps=%d interval=%dms",
            Arg::from(u32::from(s.max_packet)),
            Arg::from(u32::from(s.interval))
        );
        Ok(())
    }
}

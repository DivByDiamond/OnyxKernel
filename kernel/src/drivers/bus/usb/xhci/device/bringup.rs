//! xHCI device bring-up between controller init and enumeration: reset
//! and enable one connected port, grant a device slot and issue the
//! Address Device command so control transfers can flow. One call
//! brings up one port; the boot probe calls it once per connected port,
//! so any number of devices (up to the per-device table) coexist.
//!
//! `xhci::init` deliberately covers only the controller (rings, DCBAA,
//! RUN state); neither the port reset nor the slot enable lives there —
//! `xhci_control_transfer` refuses with `NoEnt` until a slot exists —
//! so this is the explicit bring-up the boot probe runs before
//! `dev::enumerate` (ruling 5 finding). A failure at ANY step disables
//! the granted slot and frees every page the device took.

use core::ptr;
use onyx_core::errno::{Errno, KResult};
use onyx_core::fmt::Arg;

use super::super::devs;
use super::super::pages::with_scratch_page;
use super::super::{
    G_XHCI, address_device, enable_slot, port_connect, regs, ring, xhci_port_enable,
    xhci_port_reset,
};
use super::{SPEED_FS, SPEED_HS, SPEED_LS, SPEED_SS, set_input_ctx_slot};
use crate::mm::pmm;

/// Default EP0 max packet for full/low-speed devices (USB 2.0 §9.6.6).
const MPS0_FS: u16 = 8;
/// Default EP0 max packet for high-speed devices.
const MPS0_HS: u16 = 64;
/// EP0 max packet for SuperSpeed devices.
const MPS0_SS: u16 = 512;
/// EP0 transfer ring entries.
const EP0_RING_ENTRIES: u16 = 32;

/// Bring the device on root port `port` to the addressed state: port
/// reset + enable, slot enable, a per-device context page published in
/// the DCBAA and a per-device EP0 ring, then Address Device with a
/// slot/EP0 input context. Returns the controller slot id — the
/// handle `xhci_control_transfer` and the interrupt arm/poll use.
///
/// `Errno::NoEnt` when nothing is attached to `port`, `Range` for a
/// port beyond the controller, `Busy` when a device already sits on it;
/// every step is bounded (busy-waits have fixed timeouts). On any
/// error after the slot was granted the slot is disabled again and
/// every page taken is freed: a failed bring-up leaks nothing.
///
/// # Safety
///
/// Requires a prior successful `xhci::init` (operational controller,
/// command/event rings and the DCBAA allocated); boot-probe context
/// (single-threaded, SIE=0) before secondary harts start.
pub unsafe fn bringup_device(port: u8) -> KResult<u8> {
    // SAFETY: boot-probe single-owner path per the contract; MMIO
    // offsets come from the validated `obase`, `port` is bounds-checked.
    unsafe {
        let ctx = &raw const G_XHCI;
        if !(*ctx).operational {
            return Err(Errno::NoSys);
        }
        if port >= (*ctx).max_ports {
            return Err(Errno::Range);
        }
        if !port_connect(port) {
            return Err(Errno::NoEnt);
        }
        if devs::table().iter().any(|d| d.in_use && d.port == port) {
            return Err(Errno::Busy);
        }

        // Controller init does not reset ports (ruling 5) — do it here.
        xhci_port_reset(port)?;
        xhci_port_enable(port)?;

        // Slot-context speed = the PORTSC port speed ID as-is (§6.2.2):
        // the slot-context values are 1:1 with PORTSC (FS=1, LS=2,
        // HS=3, SS=4, per §5.4.8/§6.2.2) — no offset.
        // EP0 max packet follows the device speed.
        let psc = regs::op_r32((*ctx).obase, regs::OP_PORTSC + (port as u32) * 0x10);
        let speed_id = (psc & regs::PORT_SPEED_MASK) >> regs::PORT_SPEED_SHIFT;
        let (slot_speed, mps0) = match speed_id {
            regs::PORT_SPEED_ID_FS => (SPEED_FS, MPS0_FS),
            regs::PORT_SPEED_ID_LS => (SPEED_LS, MPS0_FS),
            regs::PORT_SPEED_ID_HS => (SPEED_HS, MPS0_HS),
            _ => (SPEED_SS, MPS0_SS),
        };

        let slot_id = enable_slot()?;
        // Claim the per-device record. A claim failure means the id is
        // unusable for us (beyond the table / already live): give the
        // controller's slot back WITHOUT touching any live device.
        if let Err(e) = devs::claim(devs::table(), slot_id, port, slot_speed) {
            let _ = disable_slot(slot_id);
            return Err(e);
        }
        if let Err(e) = address_new_slot(slot_id, port, slot_speed, mps0) {
            release_device(slot_id);
            return Err(e);
        }
        crate::kinf!(
            "xhci",
            "device slot=%d port=%d speed=%d mps0=%d",
            Arg::from(slot_id as u32),
            Arg::from(port as u32),
            Arg::from(slot_speed as u32),
            Arg::from(mps0 as u32)
        );
        Ok(slot_id)
    }
}

/// Publish the device context and EP0 ring of the freshly claimed
/// `slot_id`, then run Address Device. Every page goes through the
/// device's ledger; the caller unwinds via [`release_device`] on `Err`.
///
/// # Safety
///
/// `slot_id` was just granted and claimed; boot-probe context.
unsafe fn address_new_slot(slot_id: u8, port: u8, speed: u8, mps0: u16) -> KResult<()> {
    // SAFETY: single-owner path; the DCBAA is a full page of u64
    // entries (init) and `slot_id` is within max_slots; PMM pages are
    // zeroed and identity-mapped. No table reference crosses the
    // Address Device command wait.
    unsafe {
        let (deq, dcs) = {
            let d = devs::dev(slot_id).ok_or(Errno::NoEnt)?;
            // Device-context buffer for the slot, published in the DCBAA
            // entry before Address Device — the controller writes the
            // output context there.
            let dev_ctx_pa = d.pages.take(|| pmm::alloc_zero(), |pa| pmm::free(pa))?;
            ptr::write(super::super::dcbaa().add(slot_id as usize), dev_ctx_pa);
            // EP0 ring: control transfers enqueue here; the input
            // context programs its dequeue pointer.
            let ep0_pa = d.pages.take(|| pmm::alloc_zero(), |pa| pmm::free(pa))?;
            let r = ring::init_ring(ep0_pa, EP0_RING_ENTRIES);
            let start = (r.phys, r.cycle);
            d.rings[0] = r;
            start
        };
        // Input context (control + slot + EP0) on a scratch page, freed
        // on every path once the command has completed.
        with_scratch_page(
            || pmm::alloc_zero(),
            |pa| pmm::free(pa),
            |input_pa| {
                set_input_ctx_slot(input_pa as *mut u8, port, speed, mps0, deq, dcs);
                address_device(slot_id, input_pa)
            },
        )
    }
}

/// Disable Slot command for `slot_id` (§4.6.4) and clear its DCBAA
/// entry once the controller confirmed.
///
/// # Safety
///
/// Operational controller; boot-probe/serialized single-owner path.
unsafe fn disable_slot(slot_id: u8) -> KResult<()> {
    // SAFETY: single-owner path; DCBAA index is a controller-granted id.
    unsafe {
        let mut trb = ring::Trb::zero();
        trb.set_type(ring::TRB_DISABLE_SLOT);
        trb.set_flags(ring::TRB_IOC);
        trb.set_slot_id(slot_id);
        let ev = ring::submit_command(&trb)?;
        if !ring::command_ok(&ev) {
            return Err(Errno::Io);
        }
        ptr::write(super::super::dcbaa().add(slot_id as usize), 0);
        Ok(())
    }
}

/// Tear a device down: Disable Slot, then free its context, rings and
/// buffers. If the controller does not confirm the Disable (it may
/// still DMA into those pages) the pages are deliberately KEPT and the
/// record stays claimed — a bounded leak beats a use-after-free — and a
/// kwrn is logged. Unknown / dead slot ids are a no-op.
///
/// # Safety
///
/// Operational controller; boot-probe/serialized single-owner path; no
/// transfer on this device may be in flight (the poll stops at the
/// registry level first).
pub unsafe fn release_device(slot_id: u8) {
    // SAFETY: single-owner path; the table record is released only
    // after the controller confirmed the slot is disabled.
    unsafe {
        if devs::dev(slot_id).is_none() {
            return;
        }
        match disable_slot(slot_id) {
            Ok(()) => {
                if let Some(d) = devs::dev(slot_id) {
                    d.release(|pa| pmm::free(pa));
                }
                crate::kinf!("xhci", "slot=%d released", Arg::from(u32::from(slot_id)));
            }
            Err(e) => crate::kwrn!(
                "xhci",
                "disable slot=%d failed errno=%d, pages kept",
                Arg::from(u32::from(slot_id)),
                Arg::from(e.as_i64())
            ),
        }
    }
}

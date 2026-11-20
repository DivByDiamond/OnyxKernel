//! xHCI device bring-up between controller init and enumeration: find a
//! connected port, reset and enable it, then grant a device slot and
//! issue the Address Device command so control transfers can flow.
//!
//! `xhci::init` deliberately covers only the controller (rings, DCBAA,
//! RUN state); neither the port reset nor the slot enable lives there —
//! `xhci_control_transfer` refuses with `NoEnt` until a slot exists —
//! so this is the explicit bring-up the boot probe runs before
//! `dev::enumerate` (ruling 5 finding).

use core::ptr;
use onyx_core::errno::{Errno, KResult};
use onyx_core::fmt::Arg;

use super::super::{
    G_XHCI, address_device, enable_slot, port_connect, regs, xhci_port_enable, xhci_port_reset,
};
use super::{SPEED_FS, SPEED_HS, SPEED_LS, SPEED_SS, create_ctx_array, set_input_ctx_slot};
use crate::mm::pmm;

/// Default EP0 max packet for full/low-speed devices (USB 2.0 §9.6.6).
const MPS0_FS: u16 = 8;
/// Default EP0 max packet for high-speed devices.
const MPS0_HS: u16 = 64;
/// EP0 max packet for SuperSpeed devices.
const MPS0_SS: u16 = 512;

/// Bring the first attached device to the addressed state: port reset +
/// enable, slot enable with the device-context buffer published in the
/// DCBAA, then Address Device with a slot/EP0 input context.
///
/// After this, `G_XHCI.slot` names the device and `xhci_control_transfer`
/// can carry enumeration traffic. Fails with `Errno::NoEnt` when no
/// port has a device attached; every step is bounded (busy-waits have
/// fixed timeouts).
///
/// # Safety
///
/// Requires a prior successful `xhci::init` (operational controller,
/// command/event rings and the DCBAA allocated); must run once on the
/// boot hart in the single-threaded peripheral probe before secondary
/// harts start.
pub unsafe fn bringup_device() -> KResult<()> {
    unsafe {
        let ctx = &raw const G_XHCI;
        if !(*ctx).operational {
            return Err(Errno::NoSys);
        }
        let max_ports = (*ctx).max_ports;
        let obase = (*ctx).obase;

        // First connected port (boot contract: exactly one device).
        let mut found: Option<u8> = None;
        for port in 0..max_ports {
            if port_connect(port) {
                found = Some(port);
                break;
            }
        }
        let port = found.ok_or(Errno::NoEnt)?;

        // Controller init does not reset ports (ruling 5) — do it here.
        xhci_port_reset(port)?;
        xhci_port_enable(port)?;

        // Slot-context speed = the PORTSC port speed ID as-is (§6.2.2):
        // the slot-context values are 1:1 with PORTSC (FS=1, LS=2,
        // HS=3, SS=4, per §5.4.8/§6.2.2) — no offset.
        // EP0 max packet follows the device speed.
        let psc = regs::op_r32(obase, regs::OP_PORTSC + (port as u32) * 0x10);
        let speed_id = (psc & regs::PORT_SPEED_MASK) >> regs::PORT_SPEED_SHIFT;
        let (slot_speed, mps0) = match speed_id {
            regs::PORT_SPEED_ID_FS => (SPEED_FS, MPS0_FS),
            regs::PORT_SPEED_ID_LS => (SPEED_LS, MPS0_FS),
            regs::PORT_SPEED_ID_HS => (SPEED_HS, MPS0_HS),
            _ => (SPEED_SS, MPS0_SS),
        };

        let slot_id = enable_slot()?;

        // Remember the bring-up facts the interrupt arm needs: the slot
        // context copy for Configure Endpoint (root port, speed).
        // SAFETY: single-threaded boot probe with SIE=0; G_XHCI is
        // exclusively owned on this path.
        let g = &raw mut G_XHCI;
        (*g).dev_port = port;
        (*g).dev_speed = slot_speed;

        // Device-context buffer for the slot, published in the DCBAA
        // entry before Address Device — the controller writes the output
        // context there.
        let dev_ctx_pa = create_ctx_array()?;
        // SAFETY: the DCBAA is a full page of u64 entries (init), and
        // the controller-granted slot_id is within max_slots.
        ptr::write((*ctx).dcbaap.add(slot_id as usize), dev_ctx_pa);

        // Input context (control + slot + EP0) for Address Device: one
        // zeroed page through the PMM. set_input_ctx_slot also programs
        // the EP0 ring dequeue, per init's publication contract.
        // SAFETY: PMM is initialized before the peripheral probe; the
        // returned physical page is identity-mapped and zeroed.
        let input_pa = pmm::alloc_zero()?;
        set_input_ctx_slot(input_pa as *mut u8, port, slot_speed, mps0);
        address_device(slot_id, input_pa)?;

        crate::kinf!(
            "xhci",
            "device slot=%d port=%d speed=%d mps0=%d",
            Arg::from(slot_id as u32),
            Arg::from(port as u32),
            Arg::from(slot_speed as u32),
            Arg::from(mps0 as u32)
        );
        Ok(())
    }
}

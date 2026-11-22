use core::ptr;
use onyx_core::errno::{Errno, KResult};

pub mod device;
mod devs;
pub mod init;
mod pages;
mod ports;
pub use ports::port_connect;
pub(super) use ports::{xhci_n_ports, xhci_port_enable, xhci_port_reset, xhci_port_status};
pub mod regs;
pub use regs::ring;
mod xfer;

use devs::park_if_armed;
use xfer::ack_event_ring;

pub(super) use xfer::bulk::xhci_bulk_transfer;
pub(super) use xfer::xhci_control_transfer;

pub struct XhciCtx {
    pub base: usize,
    pub obase: usize,
    /// Absolute MMIO address of the doorbell register array (BAR base +
    /// DBOFF) — `doorbell_w32` writes it as-is.
    pub dboff: usize,
    /// Absolute MMIO address of the runtime register set (BAR base +
    /// RTSOFF) — the rt_* helpers write it as-is.
    pub rtsoff: usize,
    pub cap_len: u8,
    pub hci_version: u16,
    pub max_slots: u8,
    pub max_intrs: u8,
    pub max_ports: u8,
    pub page_size: u32,
    pub dcbaap: *mut u64,
    pub cmd_ring: ring::TrbRing,
    pub event_ring: ring::EventRing,
    pub operational: bool,
}

pub(crate) static mut G_XHCI: XhciCtx = XhciCtx {
    base: 0,
    obase: 0,
    dboff: 0,
    rtsoff: 0,
    cap_len: 0,
    hci_version: 0,
    max_slots: 0,
    max_intrs: 0,
    max_ports: 0,
    page_size: 0,
    dcbaap: ptr::null_mut(),
    cmd_ring: ring::TrbRing {
        base: ptr::null_mut(),
        phys: 0,
        size: 0,
        enqueue: 0,
        cycle: false,
    },
    event_ring: ring::EventRing {
        base: ptr::null_mut(),
        phys: 0,
        size: 0,
        dequeue: 0,
        cycle: false,
    },
    operational: false,
};

pub use device::{bringup_device, release_device};
pub use init::init;
/// Interrupt-IN arm/poll for the HID stick channel (task 4).
pub use xfer::intr::xhci_arm_interrupt;
pub(super) use xfer::intr::xhci_poll_interrupt;

/// # Safety
///
/// `base` must be the MMIO base of an xHCI capability register file that was
/// probed/validated at driver init and identity-mapped at boot.
pub unsafe fn probe(base: usize) -> bool {
    // SAFETY: caller guarantees `base` lies inside the controller's MMIO
    // window; HCIVERSION offset 0x02 is within the capability registers.
    unsafe {
        let hci_ver = regs::read_hciversion(base);
        hci_ver >= 0x100
    }
}

/// # Safety
///
/// The controller must be initialized and operational (cmd/event rings
/// allocated by init); must run before secondary harts start or serialized
/// by the driver's lock.
pub unsafe fn enable_slot() -> KResult<u8> {
    // SAFETY: init allocated cmd/event rings with valid base pointers and
    // ring-size entries; submit_command keeps enqueue within the ring.
    unsafe {
        let mut trb = ring::Trb::zero();
        trb.set_type(ring::TRB_ENABLE_SLOT);
        trb.set_flags(ring::TRB_IOC);
        let ev = ring::submit_command(&trb)?;
        let slot_id = (ev.params[3] >> 24) as u8;
        if !ring::command_ok(&ev) || slot_id == 0 {
            return Err(Errno::Io);
        }
        Ok(slot_id)
    }
}

/// # Safety
///
/// Controller initialized and operational; `input_ctx_pa` must point at a
/// physically contiguous page-aligned input context buffer allocated by the
/// caller via the PMM; `slot_id` must be a slot granted by `enable_slot`.
pub unsafe fn address_device(slot_id: u8, input_ctx_pa: u64) -> KResult<()> {
    // SAFETY: init allocated the DCBAA as a page with max_slots+1 u64
    // entries; slot_id is a controller-granted slot id within that range,
    // so dcbaap.add(slot_id) stays inside the allocation.
    unsafe {
        let mut trb = ring::Trb::zero();
        trb.params[0] = input_ctx_pa as u32;
        trb.params[1] = (input_ctx_pa >> 32) as u32;
        trb.set_type(ring::TRB_ADDRESS_DEVICE);
        trb.set_flags(ring::TRB_IOC);
        // Slot id rides in the control word (§6.4.3.4), after set_type.
        trb.set_slot_id(slot_id);
        let ev = ring::submit_command(&trb)?;
        if !ring::command_ok(&ev) {
            return Err(Errno::Io);
        }
        let dcbaap = G_XHCI.dcbaap;
        let dev_ctx_pa = ptr::read(dcbaap.add(slot_id as usize));
        if dev_ctx_pa == 0 {
            return Err(Errno::Io);
        }
        Ok(())
    }
}

/// # Safety
///
/// Must only be invoked from the xHCI IRQ context on the hart that owns the
/// controller; G_XHCI must have been initialized.
pub unsafe fn irq_handler() {
    // SAFETY: read-only access to G_XHCI from IRQ context; single hart
    // services this controller's IRQ, so no concurrent mutation occurs.
    unsafe {
        let ctx = &raw const G_XHCI;
        if !(*ctx).operational {
            return;
        }
        // SAFETY: rtsoff comes from init (validated RTSOFF register value
        // within the MMIO window); RTS_IMAN is a valid runtime-register offset.
        let iman = regs::rt_r32((*ctx).rtsoff, 0, regs::RTS_IMAN);
        if (iman & regs::IMAN_IP) != 0 {
            // SAFETY: same validated rtsoff; writing IMAN_IP clears the
            // pending bit per the xHCI spec.
            regs::rt_w32((*ctx).rtsoff, 0, regs::RTS_IMAN, regs::IMAN_IP);
        }
    }
}

/// Has `init` brought the controller up? Plain read of `G_XHCI`,
/// written only by single-threaded init.
pub(crate) fn is_operational() -> bool {
    let ctx = &raw const G_XHCI;
    // SAFETY: plain read of a flag written once by boot-time init.
    unsafe { (*ctx).operational }
}

/// DCBAA base (null until `init` runs).
pub(crate) fn dcbaa() -> *mut u64 {
    let ctx = &raw const G_XHCI;
    // SAFETY: plain read of a pointer written once by boot-time init.
    unsafe { (*ctx).dcbaap }
}

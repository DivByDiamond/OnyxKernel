use core::ptr;
use onyx_core::errno::{Errno, KResult};

pub mod device;
pub mod init;
pub mod regs;
pub use regs::ring;

pub struct XhciCtx {
    pub base: usize,
    pub obase: usize,
    pub dboff: usize,
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
    pub xfer_rings: [*mut ring::TrbRing; 32],
    pub slot: u8,
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
    xfer_rings: [ptr::null_mut(); 32],
    slot: 0,
    operational: false,
};

pub use init::init;

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
/// The driver must have been initialized via `init`; G_XHCI.obase must be a
/// valid operational register base. `port` must be < max_ports.
pub unsafe fn port_connect(port: u8) -> bool {
    // SAFETY: G_XHCI.obase was set by init from the validated controller
    // base; OP_PORTSC + port*0x10 is within the operational register file.
    unsafe {
        let reg = regs::OP_PORTSC + (port as u32) * 0x10;
        let v = regs::op_r32(G_XHCI.obase, reg);
        (v & regs::PORT_CCS) != 0
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
        if slot_id == 0 {
            return Err(Errno::Io);
        }
        G_XHCI.slot = slot_id;
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
        trb.params[2] = (slot_id as u32) << 24;
        trb.set_type(ring::TRB_ADDRESS_DEVICE);
        trb.set_flags(ring::TRB_IOC);
        ring::submit_command(&trb)?;
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

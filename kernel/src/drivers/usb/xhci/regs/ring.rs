use crate::mm::pmm;
use core::ptr;
use onyx_core::errno::{Errno, KResult};

use crate::drivers::usb::xhci::XhciCtx;

pub const TRB_NORMAL: u32 = 1;
pub const TRB_SETUP: u32 = 2;
pub const TRB_DATA: u32 = 3;
pub const TRB_STATUS: u32 = 4;
pub const TRB_LINK: u32 = 6;
pub const TRB_ENABLE_SLOT: u32 = 9;
pub const TRB_ADDRESS_DEVICE: u32 = 11;
pub const TRB_CONFIG_EP: u32 = 12;
pub const TRB_EVAL_CTX: u32 = 13;
pub const TRB_CMD_NOOP: u32 = 10;
pub const TRB_CMD_COMPL: u32 = 33;
pub const TRB_PORT_STATUS: u32 = 34;
pub const TRB_TRANSFER_EVENT: u32 = 32;

pub const TRB_C: u32 = 1 << 0;
pub const TRB_ENT: u32 = 1 << 1;
pub const TRB_ISP: u32 = 1 << 2;
pub const TRB_NS: u32 = 1 << 3;
pub const TRB_CH: u32 = 1 << 4;
pub const TRB_IOC: u32 = 1 << 5;
pub const TRB_IDT: u32 = 1 << 6;
pub const TRB_BEI: u32 = 1 << 7;

fn trb_type_flags(t: u32) -> u32 {
    (t & 0x3F) << 10
}

#[repr(C, align(16))]
#[derive(Clone, Copy)]
pub struct Trb {
    pub params: [u32; 4],
}

impl Trb {
    pub const fn zero() -> Trb {
        Trb { params: [0; 4] }
    }

    pub fn set_type(&mut self, t: u32) {
        let c = self.params[3] & TRB_C;
        self.params[3] = c | trb_type_flags(t);
    }

    pub fn set_cycle(&mut self, c: bool) {
        if c {
            self.params[3] |= TRB_C;
        } else {
            self.params[3] &= !TRB_C;
        }
    }

    pub fn set_flags(&mut self, flags: u32) {
        self.params[3] |= flags;
    }
}

pub struct TrbRing {
    pub base: *mut Trb,
    pub phys: u64,
    pub size: u16,
    pub enqueue: u16,
    pub cycle: bool,
}

pub unsafe fn alloc_ring(nentries: u16) -> KResult<TrbRing> {
    let n = nentries as usize;
    let bytes = n * 16;
    let pages = (bytes + 4095) / 4096;
    let pa = pmm::alloc_n(pages)? as usize;
    let ring = TrbRing {
        base: pa as *mut Trb,
        phys: pa as u64,
        size: nentries,
        enqueue: 0,
        cycle: true,
    };
    let link = &mut *ring.base.add((nentries - 1) as usize);
    link.params[0] = pa as u32;
    link.params[1] = (pa >> 32) as u32;
    link.params[2] = 0;
    link.params[3] = TRB_C | trb_type_flags(TRB_LINK);
    Ok(ring)
}

pub unsafe fn enqueue_trb(ring: &mut TrbRing, trb: &Trb) {
    let idx = ring.enqueue as usize;
    let dst = &mut *ring.base.add(idx);
    let mut t = trb.clone();
    t.set_cycle(ring.cycle);
    ptr::write(dst, t);
    ring.enqueue += 1;
    if ring.enqueue >= ring.size - 1 {
        ring.enqueue = 0;
        ring.cycle = !ring.cycle;
    }
}

pub unsafe fn setup_trb(trb: &mut Trb, req: u8, req_type: u8, val: u16, idx: u16, len: u16) {
    trb.params[0] = (req_type as u32) | ((req as u32) << 8) | ((val as u32) << 16);
    trb.params[1] = (idx as u32) | ((len as u32) << 16);
    trb.params[2] = 0;
    trb.params[3] = trb_type_flags(TRB_SETUP);
}

pub unsafe fn ring_doorbell(slot: u8, target: u8) {
    let ctx = &raw const crate::drivers::usb::xhci::G_XHCI;
    let dboff = (*ctx).dboff;
    crate::drivers::usb::xhci::regs::doorbell_w32(dboff, slot, target);
}

pub struct EventRing {
    pub base: *mut Trb,
    pub phys: u64,
    pub size: u16,
    pub dequeue: u16,
    pub cycle: bool,
}

pub unsafe fn alloc_event_ring(nentries: u16) -> KResult<EventRing> {
    let n = nentries as usize;
    let bytes = n * 16;
    let pages = (bytes + 4095) / 4096;
    let pa = pmm::alloc_n(pages)? as usize;
    Ok(EventRing {
        base: pa as *mut Trb,
        phys: pa as u64,
        size: nentries,
        dequeue: 0,
        cycle: false,
    })
}

pub unsafe fn poll_event(er: &mut EventRing) -> Option<Trb> {
    let idx = er.dequeue as usize;
    if idx >= er.size as usize {
        return None;
    }
    let ev = &*er.base.add(idx);
    let c = (ev.params[3] & TRB_C) != 0;
    if c != er.cycle {
        return None;
    }
    let trb = ev.clone();
    er.dequeue += 1;
    if er.dequeue >= er.size {
        er.dequeue = 0;
        er.cycle = !er.cycle;
    }
    Some(trb)
}

pub unsafe fn submit_command(trb: &Trb) -> KResult<Trb> {
    let ctx = &raw mut crate::drivers::usb::xhci::G_XHCI;
    let cmd_ring = &mut (*ctx).cmd_ring;
    enqueue_trb(cmd_ring, trb);
    crate::drivers::usb::xhci::regs::doorbell_w32((*ctx).dboff, 0, 0);
    let er = &mut (*ctx).event_ring;
    let mut timeout = 1_000_000u32;
    while timeout > 0 {
        if let Some(ev) = poll_event(er) {
            let t = (ev.params[3] >> 10) & 0x3F;
            if t == TRB_CMD_COMPL {
                return Ok(ev);
            }
        }
        timeout -= 1;
    }
    Err(Errno::Io)
}

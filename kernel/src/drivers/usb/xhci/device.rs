use crate::mm::pmm;
use core::ptr;
use onyx_core::errno::{Errno, KResult};

use super::XhciCtx;
use super::regs;
use super::ring;

pub const EP_TYPE_INVALID: u8 = 0;
pub const EP_TYPE_ISO_OUT: u8 = 1;
pub const EP_TYPE_BULK_OUT: u8 = 2;
pub const EP_TYPE_INT_OUT: u8 = 3;
pub const EP_TYPE_CONTROL: u8 = 4;
pub const EP_TYPE_ISO_IN: u8 = 5;
pub const EP_TYPE_BULK_IN: u8 = 6;
pub const EP_TYPE_INT_IN: u8 = 7;

pub const SPEED_LS: u8 = 1;
pub const SPEED_FS: u8 = 2;
pub const SPEED_HS: u8 = 3;
pub const SPEED_SS: u8 = 4;

const CTX_SIZE: usize = 32;

#[repr(C)]
struct SlotCtx {
    dw0: u32,
    dw1: u32,
    dw2: u32,
    dw3: u32,
}

#[repr(C)]
struct EpCtx {
    dw0: u32,
    dw1: u32,
    dw2: u32,
    dw3: u32,
}

#[repr(C)]
struct InputCtx {
    drop_flags: u32,
    add_flags: u32,
    rsvd: [u32; 6],
}

pub struct UsbDevCtx {
    pub slot_id: u8,
    pub port: u8,
    pub speed: u8,
    pub device_desc: [u8; 18],
    pub config_desc: [u8; 9],
    pub configured: bool,
}

pub unsafe fn create_ctx_array() -> KResult<u64> {
    let ctx_slots = 32;
    let total = ctx_slots as usize * CTX_SIZE;
    let pages = (total + 4095) / 4096;
    pmm::alloc_n(pages)
}

pub unsafe fn set_slot_ctx(dev_ctx: *mut u8, _slot_id: u8, port: u8, speed: u8, mps0: u16) {
    let p = dev_ctx as *mut u8;
    let sc = &mut *(p as *mut SlotCtx);
    sc.dw0 = (speed as u32) << 20 | 1 << 27 | (port as u32);
    sc.dw1 = 0;
    sc.dw2 = 0;
    sc.dw3 = 0;
    let ep0 = &mut *(p.add(CTX_SIZE) as *mut EpCtx);
    ep0.dw0 = (mps0 as u32) << 16 | (EP_TYPE_CONTROL as u32) << 3 | (3 << 1);
    ep0.dw1 = 0;
    ep0.dw2 = 0;
    ep0.dw3 = 0;
}

pub unsafe fn set_ep_ctx(dev_ctx: *mut u8, ep_idx: u8, ep_type: u8, mps: u16, deq: u64, dcs: bool) {
    if ep_idx as usize >= 32 {
        return;
    }
    let off = (ep_idx as usize + 1) * CTX_SIZE;
    let ep = &mut *(dev_ctx.add(off) as *mut EpCtx);
    ep.dw0 = (mps as u32) << 16 | (ep_type as u32) << 3 | (3 << 1);
    ep.dw1 = (deq as u32) & !0xF;
    if dcs {
        ep.dw1 |= 1;
    }
    ep.dw2 = (deq >> 32) as u32;
    ep.dw3 = mps as u32;
}

pub unsafe fn set_input_ctx_slot(input: *mut u8, port: u8, speed: u8, mps0: u16) {
    let ic = &mut *(input as *mut InputCtx);
    ic.drop_flags = 0;
    ic.add_flags = 3;
    let sc = &mut *(input.add(32) as *mut SlotCtx);
    sc.dw0 = (speed as u32) << 20 | 1 << 27 | (port as u32);
    sc.dw1 = 0;
    sc.dw2 = 0;
    sc.dw3 = 0;
    let ep0 = &mut *(input.add(64) as *mut EpCtx);
    ep0.dw0 = (mps0 as u32) << 16 | (EP_TYPE_CONTROL as u32) << 3 | (3 << 1);
    ep0.dw1 = 0;
    ep0.dw2 = 0;
    ep0.dw3 = 0;
}

pub unsafe fn set_input_ctx_ep(input: *mut u8, ep_idx: u8, ep_type: u8, mps: u16, deq: u64) {
    let ic = &mut *(input as *mut InputCtx);
    ic.add_flags |= 1u32.wrapping_shl(ep_idx as u32 + 1);
    let off = 32 + (ep_idx as usize + 1) * CTX_SIZE;
    let ep = &mut *(input.add(off) as *mut EpCtx);
    ep.dw0 = (mps as u32) << 16 | (ep_type as u32) << 3 | (3 << 1);
    ep.dw1 = (deq as u32) & !0xF;
    ep.dw2 = (deq >> 32) as u32;
    ep.dw3 = mps as u32;
}

pub unsafe fn configure_endpoint(slot_id: u8, ep_idx: u8, ep_type: u8, mps: u16) -> KResult<()> {
    let xfer_ring = ring::alloc_ring(32)?;
    let deq = xfer_ring.phys;
    let dcs = xfer_ring.cycle;

    let ring_ptr = pmm::alloc_zero()? as *mut ring::TrbRing;
    ptr::write(ring_ptr, xfer_ring);
    let ctx = &raw mut super::G_XHCI;
    (*ctx).xfer_rings[ep_idx as usize] = ring_ptr;

    let input_pa = pmm::alloc_n(2)? as usize;
    ptr::write_bytes(input_pa as *mut u8, 0, 8192);
    let deq_val = deq | if dcs { 1 } else { 0 };
    set_input_ctx_ep(input_pa as *mut u8, ep_idx, ep_type, mps, deq_val);
    let mut trb = ring::Trb::zero();
    trb.params[0] = input_pa as u32;
    trb.params[1] = (input_pa >> 32) as u32;
    trb.params[2] = (slot_id as u32) << 24;
    trb.set_type(ring::TRB_CONFIG_EP);
    trb.set_flags(ring::TRB_IOC);
    ring::submit_command(&trb)?;
    Ok(())
}

pub unsafe fn reset_device(slot_id: u8) -> KResult<()> {
    let mut trb = ring::Trb::zero();
    trb.params[0] = (slot_id as u32) << 24;
    trb.set_type(ring::TRB_EVAL_CTX);
    trb.set_flags(ring::TRB_IOC);
    ring::submit_command(&trb)?;
    Ok(())
}

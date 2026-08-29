use crate::mm::pmm;
use core::ptr;
use onyx_core::errno::KResult;

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

/// # Safety
///
/// No preconditions beyond PMM availability; safe to call once per device
/// during device setup under the driver's init path (single-threaded with
/// SIE=0).
pub unsafe fn create_ctx_array() -> KResult<u64> {
    // SAFETY: this only performs a PMM allocation; the unsafe op is the
    // pmm call whose contract is honored (page-aligned, physically
    // contiguous allocation of `pages` pages).
    unsafe {
        let ctx_slots = 32;
        let total = ctx_slots as usize * CTX_SIZE;
        let pages = total.div_ceil(4096);
        pmm::alloc_n(pages)
    }
}

/// # Safety
///
/// `dev_ctx` must point at a physically contiguous device context array of
/// at least 33 32-byte contexts (CTX_SIZE) allocated by `create_ctx_array`;
/// caller must have exclusive access (driver init path, single-threaded).
pub unsafe fn set_slot_ctx(dev_ctx: *mut u8, _slot_id: u8, port: u8, speed: u8, mps0: u16) {
    // SAFETY: caller guarantees dev_ctx has >= 2 CTX_SIZE-sized contexts,
    // so the SlotCtx at offset 0 and EpCtx at CTX_SIZE are in bounds.
    unsafe {
        let p = dev_ctx;
        // SAFETY: aligned u32 fields of the in-bounds SlotCtx (offset 0).
        let sc = &mut *(p as *mut SlotCtx);
        sc.dw0 = (speed as u32) << 20 | 1 << 27 | (port as u32);
        sc.dw1 = 0;
        sc.dw2 = 0;
        sc.dw3 = 0;
        // SAFETY: offset CTX_SIZE is the second context, within the
        // caller-provided buffer.
        let ep0 = &mut *(p.add(CTX_SIZE) as *mut EpCtx);
        ep0.dw0 = (mps0 as u32) << 16 | (EP_TYPE_CONTROL as u32) << 3 | (3 << 1);
        ep0.dw1 = 0;
        ep0.dw2 = 0;
        ep0.dw3 = 0;
    }
}

/// # Safety
///
/// `dev_ctx` must point at a device context array of >= 33 CTX_SIZE-sized
/// contexts; `ep_idx` must be < 32 (checked here); caller must have
/// exclusive access.
pub unsafe fn set_ep_ctx(dev_ctx: *mut u8, ep_idx: u8, ep_type: u8, mps: u16, deq: u64, dcs: bool) {
    // SAFETY: ep_idx < 32 is checked above, so offset (ep_idx+1)*CTX_SIZE
    // is within the caller-provided context array.
    unsafe {
        if ep_idx as usize >= 32 {
            return;
        }
        let off = (ep_idx as usize + 1) * CTX_SIZE;
        // SAFETY: in-bounds per the ep_idx bound check above.
        let ep = &mut *(dev_ctx.add(off) as *mut EpCtx);
        ep.dw0 = (mps as u32) << 16 | (ep_type as u32) << 3 | (3 << 1);
        ep.dw1 = (deq as u32) & !0xF;
        if dcs {
            ep.dw1 |= 1;
        }
        ep.dw2 = (deq >> 32) as u32;
        ep.dw3 = mps as u32;
    }
}

/// # Safety
///
/// `input` must point at a physically contiguous input context block of at
/// least 3 CTX_SIZE-sized contexts (control + slot + EP0); caller must have
/// exclusive access (driver init path).
pub unsafe fn set_input_ctx_slot(input: *mut u8, port: u8, speed: u8, mps0: u16) {
    // SAFETY: caller guarantees the buffer covers offsets 0..=96
    // (InputCtx + SlotCtx + EpCtx at 32/64), so all writes are in bounds.
    unsafe {
        // SAFETY: offset 0, in bounds per contract.
        let ic = &mut *(input as *mut InputCtx);
        ic.drop_flags = 0;
        ic.add_flags = 3;
        // SAFETY: offset 32 = slot context, in bounds per contract.
        let sc = &mut *(input.add(32) as *mut SlotCtx);
        sc.dw0 = (speed as u32) << 20 | 1 << 27 | (port as u32);
        sc.dw1 = 0;
        sc.dw2 = 0;
        sc.dw3 = 0;
        // SAFETY: offset 64 = EP0 context, in bounds per contract.
        let ep0 = &mut *(input.add(64) as *mut EpCtx);
        ep0.dw0 = (mps0 as u32) << 16 | (EP_TYPE_CONTROL as u32) << 3 | (3 << 1);
        ep0.dw1 = 0;
        ep0.dw2 = 0;
        ep0.dw3 = 0;
    }
}

/// # Safety
///
/// `input` must point at a physically contiguous input context block large
/// enough for context index ep_idx+1 (i.e. >= 32 + (ep_idx+2)*CTX_SIZE
/// bytes; 8 KiB as allocated by `configure_endpoint` covers all ep_idx);
/// `ep_idx` must be < 31 for a valid xHCI context layout; caller must have
/// exclusive access.
pub unsafe fn set_input_ctx_ep(input: *mut u8, ep_idx: u8, ep_type: u8, mps: u16, deq: u64) {
    // SAFETY: caller guarantees the buffer spans the context at offset
    // 32 + (ep_idx+1)*CTX_SIZE and holds exclusive access.
    unsafe {
        // SAFETY: offset 0 = input control context, in bounds per contract.
        let ic = &mut *(input as *mut InputCtx);
        ic.add_flags |= 1u32.wrapping_shl(ep_idx as u32 + 1);
        let off = 32 + (ep_idx as usize + 1) * CTX_SIZE;
        // SAFETY: in bounds per the caller contract above.
        let ep = &mut *(input.add(off) as *mut EpCtx);
        ep.dw0 = (mps as u32) << 16 | (ep_type as u32) << 3 | (3 << 1);
        ep.dw1 = (deq as u32) & !0xF;
        ep.dw2 = (deq >> 32) as u32;
        ep.dw3 = mps as u32;
    }
}

/// # Safety
///
/// Controller initialized and operational (cmd/event rings allocated by
/// init); `ep_idx` must be < 31 (xHCI has 31 usable endpoint contexts
/// beyond ep0) and unique per slot; must be called from the driver's
/// single-owner init path (no other hart touches the command ring).
pub unsafe fn configure_endpoint(slot_id: u8, ep_idx: u8, ep_type: u8, mps: u16) -> KResult<()> {
    // SAFETY: init path with exclusive access to the static driver state;
    // ep_idx < 32 is the caller contract for the 32-entry xfer_rings array.
    unsafe {
        let xfer_ring = ring::alloc_ring(32)?;
        let deq = xfer_ring.phys;
        let dcs = xfer_ring.cycle;

        // SAFETY: ring_ptr is a fresh PMM allocation sized for TrbRing;
        // write fully initializes it before it is published.
        let ring_ptr = pmm::alloc_zero()? as *mut ring::TrbRing;
        ptr::write(ring_ptr, xfer_ring);
        let ctx = &raw mut super::G_XHCI;
        // SAFETY: ep_idx < 32 per caller contract, so this stays within
        // the 32-entry xfer_rings array; exclusive access held.
        (*ctx).xfer_rings[ep_idx as usize] = ring_ptr;

        // SAFETY: input_pa is a 2-page zeroed PMM allocation; zeroing and
        // writing via set_input_ctx_ep (ep_idx < 31 contract) stay in bounds.
        let input_pa = pmm::alloc_n(2)? as usize;
        ptr::write_bytes(input_pa as *mut u8, 0, 8192);
        let deq_val = deq | if dcs { 1 } else { 0 };
        set_input_ctx_ep(input_pa as *mut u8, ep_idx, ep_type, mps, deq_val);
        let mut trb = ring::Trb::zero();
        trb.params[0] = input_pa as u32;
        // xHCI TRBs carry 64-bit addresses even though this target can
        // only physically address 32 bits; the high word is simply zero.
        #[cfg(target_pointer_width = "64")]
        {
            trb.params[1] = (input_pa >> 32) as u32;
        }
        #[cfg(target_pointer_width = "32")]
        {
            trb.params[1] = 0;
        }
        trb.params[2] = (slot_id as u32) << 24;
        trb.set_type(ring::TRB_CONFIG_EP);
        trb.set_flags(ring::TRB_IOC);
        ring::submit_command(&trb)?;
        Ok(())
    }
}

/// # Safety
///
/// Controller initialized and operational; `slot_id` must be a slot granted
/// by `enable_slot`; single-owner driver init path.
pub unsafe fn reset_device(slot_id: u8) -> KResult<()> {
    // SAFETY: init allocated cmd/event rings; submit_command keeps enqueue
    // within the allocated ring and rings the doorbell safely.
    unsafe {
        let mut trb = ring::Trb::zero();
        trb.params[0] = (slot_id as u32) << 24;
        trb.set_type(ring::TRB_EVAL_CTX);
        trb.set_flags(ring::TRB_IOC);
        ring::submit_command(&trb)?;
        Ok(())
    }
}

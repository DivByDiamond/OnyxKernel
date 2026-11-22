#[cfg(test)]
mod tests;

mod bringup;
pub(crate) mod ctx;
pub use bringup::{bringup_device, release_device};
use ctx::{CERR, EpCtx, InputCtx, SlotCtx, slot_ctx_root_port};

pub const EP_TYPE_INVALID: u8 = 0;
pub const EP_TYPE_ISO_OUT: u8 = 1;
pub const EP_TYPE_BULK_OUT: u8 = 2;
pub const EP_TYPE_INT_OUT: u8 = 3;
pub const EP_TYPE_CONTROL: u8 = 4;
pub const EP_TYPE_ISO_IN: u8 = 5;
pub const EP_TYPE_BULK_IN: u8 = 6;
pub const EP_TYPE_INT_IN: u8 = 7;

/// Slot-context DW0 speed-field values (xHCI §6.2.2): the PORTSC port
/// speed ID, 1:1 — FS=1, LS=2, HS=3, SS=4 (per §5.4.8/§6.2.2);
/// 0 is reserved in slot contexts.
pub const SPEED_FS: u8 = 1;
pub const SPEED_LS: u8 = 2;
pub const SPEED_HS: u8 = 3;
pub const SPEED_SS: u8 = 4;

/// # Safety
///
/// `input` must point at a physically contiguous input context block of at
/// least 3 CTX_SIZE-sized contexts (control + slot + EP0); caller must have
/// exclusive access (driver init path). `ep0_deq`/`ep0_dcs` are the
/// device's EP0 ring start and cycle state. The slot-context encoding (route
/// string, root hub port) is documented on `ctx::slot_ctx_root_port`.
pub unsafe fn set_input_ctx_slot(
    input: *mut u8,
    port: u8,
    speed: u8,
    mps0: u16,
    ep0_deq: u64,
    ep0_dcs: bool,
) {
    // SAFETY: caller guarantees the buffer covers offsets 0..=96
    // (InputCtx + SlotCtx + EpCtx at 32/64), so all writes are in bounds.
    unsafe {
        // SAFETY: offset 0, in bounds per contract.
        let ic = &mut *(input as *mut InputCtx);
        ic.drop_flags = 0;
        ic.add_flags = 3;
        // SAFETY: offset 32 = slot context, in bounds per contract.
        let sc = &mut *(input.add(32) as *mut SlotCtx);
        slot_ctx_root_port(sc, port, speed, 1);
        // SAFETY: offset 64 = EP0 context, in bounds per contract.
        let ep0 = &mut *(input.add(64) as *mut EpCtx);
        // EP0 dequeue from the device's own EP0 ring: Address Device
        // copies it into the output context. DCS mirrors its cycle state.
        let (deq, dcs) = (ep0_deq, ep0_dcs);
        // EP0 per §6.2.3: Interval 0; mps0/Control type/CErr in DW1;
        // Average TRB Length 8 in DW4 (Linux parity).
        ep0.dw0 = 0;
        ep0.dw1 = (mps0 as u32) << 16 | (EP_TYPE_CONTROL as u32) << 3 | CERR << 1;
        ep0.dw2 = (deq as u32 & !0xF) | u32::from(dcs);
        ep0.dw3 = (deq >> 32) as u32;
        ep0.dw4 = 8;
    }
}

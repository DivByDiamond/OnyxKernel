//! xHCI device-context layouts: slot, endpoint and input-control contexts.

#[repr(C)]
pub(super) struct SlotCtx {
    pub(super) dw0: u32,
    pub(super) dw1: u32,
    pub(super) dw2: u32,
    pub(super) dw3: u32,
}

#[repr(C)]
pub(super) struct EpCtx {
    pub(super) dw0: u32,
    pub(super) dw1: u32,
    pub(super) dw2: u32,
    pub(super) dw3: u32,
}

#[repr(C)]
pub(super) struct InputCtx {
    pub(super) drop_flags: u32,
    pub(super) add_flags: u32,
    pub(super) rsvd: [u32; 6],
}

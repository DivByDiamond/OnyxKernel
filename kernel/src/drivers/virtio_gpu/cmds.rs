//! virtio-gpu 2D control-command layouts and constants.

pub(super) const C_RESOURCE_CREATE_2D: u32 = 0x101;
pub(super) const C_SET_SCANOUT: u32 = 0x103;
pub(super) const C_FLUSH_RESOURCE: u32 = 0x104;

#[derive(Clone, Copy)]
#[repr(C)]
pub(crate) struct GpuCtrlHdr {
    pub hdr_type: u32,
    pub flags: u32,
    pub fence_id: u64,
    pub ctx_id: u32,
    pub padding: u32,
}

#[repr(C)]
pub(super) struct Create2D {
    pub(super) hdr: GpuCtrlHdr,
    pub(super) resource_id: u32,
    pub(super) format: u32,
    pub(super) width: u32,
    pub(super) height: u32,
}
#[repr(C)]
pub(super) struct SetScanout {
    pub(super) hdr: GpuCtrlHdr,
    pub(super) rect_x: u32,
    pub(super) rect_y: u32,
    pub(super) rect_w: u32,
    pub(super) rect_h: u32,
    pub(super) scanout_id: u32,
    pub(super) resource_id: u32,
}

/// # Safety
///
/// The body performs no unsafe operations, so the caller contract is empty;
/// the function is marked `unsafe` for symmetry with the other raw-pointer
/// GPU command helpers.
pub(super) unsafe fn hdr(t: u32) -> GpuCtrlHdr {
    GpuCtrlHdr {
        hdr_type: t,
        flags: 0,
        fence_id: 0,
        ctx_id: 0,
        padding: 0,
    }
}

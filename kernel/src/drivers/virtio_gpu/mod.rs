use crate::drivers::virtio::{
    R_DEVICE_ID, R_GUEST_FEATURES, R_HOST_FEATURES, R_MAGIC_VALUE, R_STATUS, R_VERSION,
    VIRTIO_S_ACK, VIRTIO_S_DRIVER, VIRTIO_S_DRIVER_OK, VIRTIO_S_FEATURES_OK, VqAvail, VqDesc,
    VqUsed, reg_r, reg_w,
};
use crate::mm::pmm;
use core::ptr;
use onyx_core::errno::{Errno, KResult};

pub const VIRTIO_ID_GPU: u32 = 16;
pub const GPU_WIDTH: usize = 1280;
pub const GPU_HEIGHT: usize = 720;

const C_RESOURCE_CREATE_2D: u32 = 0x101;
const C_SET_SCANOUT: u32 = 0x10B;
const C_FLUSH_RESOURCE: u32 = 0x10C;

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
struct Create2D {
    hdr: GpuCtrlHdr,
    resource_id: u32,
    format: u32,
    width: u32,
    height: u32,
}
#[repr(C)]
struct SetScanout {
    hdr: GpuCtrlHdr,
    rect_x: u32,
    rect_y: u32,
    rect_w: u32,
    rect_h: u32,
    resource_id: u32,
}

#[derive(Clone, Copy)]
pub(crate) struct VirtioGpuDev {
    pub base: usize,
    pub modern: bool,
    pub desc: *mut VqDesc,
    pub avail: *mut VqAvail,
    pub used: *mut VqUsed,
    pub last_used: u16,
    pub fb: *mut u8,
    pub width: u32,
    pub height: u32,
}

pub(crate) static mut G_GPU: VirtioGpuDev = VirtioGpuDev {
    base: 0,
    modern: false,
    desc: ptr::null_mut(),
    avail: ptr::null_mut(),
    used: ptr::null_mut(),
    last_used: 0,
    fb: ptr::null_mut(),
    width: 0,
    height: 0,
};

pub unsafe fn probe(base: usize) -> bool {
    reg_r(base, R_MAGIC_VALUE) == 0x7472_6976 && reg_r(base, R_DEVICE_ID) == VIRTIO_ID_GPU
}

unsafe fn hdr(t: u32) -> GpuCtrlHdr {
    GpuCtrlHdr {
        hdr_type: t,
        flags: 0,
        fence_id: 0,
        ctx_id: 0,
        padding: 0,
    }
}

pub unsafe fn init(base: usize, width: u32, height: u32) -> KResult<()> {
    if G_GPU.base != 0 {
        return Err(Errno::Busy);
    }
    let modern = reg_r(base, R_VERSION) >= 2;
    G_GPU.base = base;
    G_GPU.modern = modern;
    G_GPU.width = width;
    G_GPU.height = height;
    G_GPU.last_used = 0;
    reg_w(base, R_STATUS, 0);
    reg_w(base, R_STATUS, VIRTIO_S_ACK | VIRTIO_S_DRIVER);
    reg_w(
        base,
        R_GUEST_FEATURES,
        reg_r(base, R_HOST_FEATURES) & 0x1FFF_FFFF,
    );
    if modern {
        reg_w(
            base,
            R_STATUS,
            VIRTIO_S_ACK | VIRTIO_S_DRIVER | VIRTIO_S_FEATURES_OK,
        );
        if reg_r(base, R_STATUS) & VIRTIO_S_FEATURES_OK == 0 {
            return Err(Errno::Inval);
        }
    }
    xfer::setup_queue(
        &raw mut G_GPU.desc,
        &raw mut G_GPU.avail,
        &raw mut G_GPU.used,
        base,
    )?;
    reg_w(
        base,
        R_STATUS,
        VIRTIO_S_ACK | VIRTIO_S_DRIVER | VIRTIO_S_DRIVER_OK,
    );
    let fb_pages = (width as usize * height as usize * 4 + 4095) / 4096;
    let fb_pa = pmm::alloc_n(fb_pages)? as *mut u8;
    G_GPU.fb = fb_pa;
    let rid = 1u32;
    xfer::cmd_create2d(
        G_GPU.desc,
        G_GPU.avail,
        G_GPU.used,
        &raw mut G_GPU.last_used,
        base,
        rid,
        width,
        height,
    )?;
    xfer::cmd_attach(
        G_GPU.desc,
        G_GPU.avail,
        G_GPU.used,
        &raw mut G_GPU.last_used,
        base,
        rid,
        fb_pa as u32,
        width * height * 4,
    )?;
    xfer::cmd_scanout(
        G_GPU.desc,
        G_GPU.avail,
        G_GPU.used,
        &raw mut G_GPU.last_used,
        base,
        rid,
        width,
        height,
    )?;
    xfer::send_cmd(
        G_GPU.desc,
        G_GPU.avail,
        G_GPU.used,
        &raw mut G_GPU.last_used,
        base,
        &hdr(C_FLUSH_RESOURCE) as *const _ as *mut u8,
        24,
    )?;
    Ok(())
}

pub fn fb_addr() -> *mut u8 {
    unsafe { G_GPU.fb }
}
pub fn fb_size() -> usize {
    unsafe { G_GPU.width as usize * G_GPU.height as usize * 4 }
}

pub mod xfer;

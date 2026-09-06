use crate::drivers::virtio::{
    R_DEVICE_ID, R_GUEST_FEATURES, R_GUEST_FEATURES_SEL, R_HOST_FEATURES, R_HOST_FEATURES_SEL,
    R_MAGIC_VALUE, R_STATUS, R_VERSION, VIRTIO_F_VERSION_1, VIRTIO_S_ACK, VIRTIO_S_DRIVER,
    VIRTIO_S_DRIVER_OK, VIRTIO_S_FEATURES_OK, VqAvail, VqDesc, VqUsed, reg_r, reg_w,
};
use crate::mm::pmm;
use core::ptr;
use onyx_core::errno::{Errno, KResult};
use onyx_core::fmt::Arg;

pub const VIRTIO_ID_GPU: u32 = 16;
pub const GPU_WIDTH: usize = 1280;
pub const GPU_HEIGHT: usize = 720;

const C_RESOURCE_CREATE_2D: u32 = 0x101;
const C_SET_SCANOUT: u32 = 0x103;
const C_FLUSH_RESOURCE: u32 = 0x104;

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
    scanout_id: u32,
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

/// # Safety
///
/// `base` must be a candidate virtio-mmio base from the FDT probe or the
/// QEMU virt fallback constants (identity-mapped at boot).
pub unsafe fn probe(base: usize) -> bool {
    // SAFETY: base is a candidate virtio-mmio base from the boot-time probe; reg_r reads only spec offsets (magic, device ID).
    unsafe {
        reg_r(base, R_MAGIC_VALUE) == 0x7472_6976 && reg_r(base, R_DEVICE_ID) == VIRTIO_ID_GPU
    }
}

/// # Safety
///
/// The body performs no unsafe operations, so the caller contract is empty;
/// the function is marked `unsafe` for symmetry with the other raw-pointer
/// GPU command helpers.
unsafe fn hdr(t: u32) -> GpuCtrlHdr {
    GpuCtrlHdr {
        hdr_type: t,
        flags: 0,
        fence_id: 0,
        ctx_id: 0,
        padding: 0,
    }
}

/// # Safety
///
/// `base` must be a probed virtio-gpu MMIO base; must be called during the
/// single-threaded boot-time device probe, once per base.
pub unsafe fn init(base: usize, width: u32, height: u32) -> KResult<()> {
    // SAFETY: boot-time single-threaded probe (SIE=0, see crate::sync) on a probed base, Busy-guarded so G_GPU is written at most once; raw pointers passed to xfer helpers point at 'static G_GPU fields; fb_pa is a pmm::alloc_n(fb_pages) framebuffer of width*height*4 bytes.
    unsafe {
        if G_GPU.base != 0 {
            return Err(Errno::Busy);
        }
        let ver = reg_r(base, R_VERSION);
        let modern = ver >= 2;
        crate::kinf!(
            "virtio-gpu",
            "probe ver=%d modern=%d",
            onyx_core::fmt::Arg::from(ver),
            onyx_core::fmt::Arg::from(modern as u32)
        );
        G_GPU.base = base;
        G_GPU.modern = modern;
        G_GPU.width = width;
        G_GPU.height = height;
        G_GPU.last_used = 0;
        reg_w(base, R_STATUS, 0);
        reg_w(base, R_STATUS, VIRTIO_S_ACK | VIRTIO_S_DRIVER);
        // 64-bit feature negotiation via sel registers (mirror virtio-blk's correct path),
        // but only advertise VERSION_1 for modern transports — legacy devices
        // reject it and the control queue never starts.
        reg_w(base, R_HOST_FEATURES_SEL, 1);
        let host_hi = reg_r(base, R_HOST_FEATURES);
        reg_w(base, R_HOST_FEATURES_SEL, 0);
        let host_lo = reg_r(base, R_HOST_FEATURES);
        let mut guest_hi = host_hi;
        if modern {
            guest_hi |= VIRTIO_F_VERSION_1;
        }
        reg_w(base, R_GUEST_FEATURES_SEL, 0);
        reg_w(base, R_GUEST_FEATURES, host_lo & 0x1FFF_FFFF);
        reg_w(base, R_GUEST_FEATURES_SEL, 1);
        reg_w(base, R_GUEST_FEATURES, guest_hi);
        reg_w(base, R_GUEST_FEATURES_SEL, 0);
        if modern {
            reg_w(
                base,
                R_STATUS,
                VIRTIO_S_ACK | VIRTIO_S_DRIVER | VIRTIO_S_FEATURES_OK,
            );
            if reg_r(base, R_STATUS) & VIRTIO_S_FEATURES_OK == 0 {
                crate::kwrn!("virtio-gpu", "device did not set FEATURES_OK, continuing");
            }
        }
        xfer::setup_queue(
            &raw mut G_GPU.desc,
            &raw mut G_GPU.avail,
            &raw mut G_GPU.used,
            base,
        )?;
        if modern {
            reg_w(
                base,
                R_STATUS,
                VIRTIO_S_ACK | VIRTIO_S_DRIVER | VIRTIO_S_FEATURES_OK | VIRTIO_S_DRIVER_OK,
            );
        } else {
            reg_w(
                base,
                R_STATUS,
                VIRTIO_S_ACK | VIRTIO_S_DRIVER | VIRTIO_S_DRIVER_OK,
            );
        }
        crate::kinf!(
            "virtio-gpu",
            "status after DRIVER_OK=%x",
            Arg::from(reg_r(base, R_STATUS))
        );
        let fb_pages = (width as usize * height as usize * 4).div_ceil(4096);
        let fb_pa = pmm::alloc_n(fb_pages)? as *mut u8;
        G_GPU.fb = fb_pa;
        let rid = 1u32;
        if let Err(e) = xfer::cmd_create2d(
            G_GPU.desc,
            G_GPU.avail,
            G_GPU.used,
            &raw mut G_GPU.last_used,
            base,
            rid,
            width,
            height,
        ) {
            crate::kwrn!(
                "virtio-gpu",
                "create2d failed err=%d",
                Arg::from(e.as_i64())
            );
            return Err(e);
        }
        crate::kinf!("virtio-gpu", "create2d ok");
        if let Err(e) = xfer::cmd_attach(
            G_GPU.desc,
            G_GPU.avail,
            G_GPU.used,
            &raw mut G_GPU.last_used,
            base,
            rid,
            fb_pa as u32,
            width * height * 4,
        ) {
            crate::kwrn!("virtio-gpu", "attach failed err=%d", Arg::from(e.as_i64()));
            return Err(e);
        }
        crate::kinf!("virtio-gpu", "attach ok");
        if let Err(e) = xfer::cmd_scanout(
            G_GPU.desc,
            G_GPU.avail,
            G_GPU.used,
            &raw mut G_GPU.last_used,
            base,
            rid,
            width,
            height,
        ) {
            crate::kwrn!("virtio-gpu", "scanout failed err=%d", Arg::from(e.as_i64()));
            return Err(e);
        }
        crate::kinf!("virtio-gpu", "scanout ok");
        // Flush is a simple 24-byte header command — allocate a pmm page for DMA visibility.
        let flush_buf = pmm::alloc_zero()? as usize;
        {
            let h = hdr(C_FLUSH_RESOURCE);
            core::ptr::copy_nonoverlapping(&h as *const _ as *const u8, flush_buf as *mut u8, 24);
        }
        if let Err(e) = xfer::send_cmd(
            G_GPU.desc,
            G_GPU.avail,
            G_GPU.used,
            &raw mut G_GPU.last_used,
            base,
            flush_buf as *mut u8,
            24,
        ) {
            crate::kwrn!("virtio-gpu", "flush failed err=%d", Arg::from(e.as_i64()));
            return Err(e);
        }
        crate::kinf!("virtio-gpu", "flush ok");
        Ok(())
    }
}

pub fn fb_addr() -> *mut u8 {
    // SAFETY: read of the fb pointer written during single-threaded boot init; kernel code never runs with SIE set (see crate::sync).
    unsafe { G_GPU.fb }
}
pub fn fb_size() -> usize {
    // SAFETY: reads of width/height written during single-threaded boot init; kernel code never runs with SIE set (see crate::sync).
    unsafe { G_GPU.width as usize * G_GPU.height as usize * 4 }
}

pub mod xfer;

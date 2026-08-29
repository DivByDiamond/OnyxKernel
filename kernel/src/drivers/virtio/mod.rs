use crate::arch::mmio::Mmio;
use core::ptr;

pub const VIRTIO_MAX_DEVS: usize = 8;
pub const VIRTIO_BLK_SECTOR: usize = 512;
pub const VIRTQ_SIZE: usize = 256;
pub const VIRTIO_F_VERSION_1: u32 = 1;
pub const R_MAGIC_VALUE: u32 = 0x00;
pub const R_VERSION: u32 = 0x04;
pub const R_DEVICE_ID: u32 = 0x08;
// Modern virtio mmio (version 2): the 64-bit feature words are selected
// through DEVICE_FEATURES_SEL (0x14) / DRIVER_FEATURES_SEL (0x24) and
// read/written at 0x10/0x20. Reading 0x14 returns 0 — it is write-only.
pub const R_HOST_FEATURES: u32 = 0x10;
pub const R_HOST_FEATURES_SEL: u32 = 0x14;
pub const R_GUEST_FEATURES: u32 = 0x20;
pub const R_GUEST_FEATURES_SEL: u32 = 0x24;
pub const R_QUEUE_SEL: u32 = 0x30;
pub const R_QUEUE_NUM_MAX: u32 = 0x34;
pub const R_QUEUE_NUM: u32 = 0x38;
// Legacy-only: guest page size whose base-2 log becomes the shift QEMU-style
// devices apply to QUEUE_PFN. MUST be programmed before QUEUE_PFN on a freshly
// reset device — after power-on/reset the shift defaults to 0, so a PFN of
// e.g. 0x80673 would be taken literally as desc addr 0x80673 instead of
// 0x80673000 (observed under OpenSBI fw_jump where no bootloader pre-programs
// this register; OnyxBoot masked the bug by writing 4096 itself).
pub const R_GUEST_PAGE_SIZE: u32 = 0x28;
pub const R_QUEUE_ALIGN: u32 = 0x3C;
pub const R_QUEUE_PFN: u32 = 0x40;
pub const R_QUEUE_READY: u32 = 0x44;
pub const R_QUEUE_NOTIFY: u32 = 0x50;
pub const R_STATUS: u32 = 0x70;
pub const R_QUEUE_DESC_LOW: u32 = 0x80;
pub const R_QUEUE_DESC_HIGH: u32 = 0x84;
pub const R_QUEUE_AVAIL_LOW: u32 = 0x90;
pub const R_QUEUE_AVAIL_HIGH: u32 = 0x94;
pub const R_QUEUE_USED_LOW: u32 = 0xA0;
pub const R_QUEUE_USED_HIGH: u32 = 0xA4;
pub const VIRTIO_S_ACK: u32 = 1;
pub const VIRTIO_S_DRIVER: u32 = 2;
pub const VIRTIO_S_DRIVER_OK: u32 = 4;
pub const VIRTIO_S_FEATURES_OK: u32 = 8;
pub const VIRTIO_ID_BLK: u32 = 2;
pub const VIRTIO_BLK_T_IN: u32 = 0;
pub const VIRTIO_BLK_T_OUT: u32 = 1;
pub const VIRTIO_BLK_S_OK: u8 = 0;
pub const VIRTIO_BLK_S_IOERR: u8 = 1;
pub const VQ_DESC_F_NEXT: u16 = 1;
pub const VQ_DESC_F_WRITE: u16 = 2;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VqDesc {
    pub addr: u64,
    pub len: u32,
    pub flags: u16,
    pub next: u16,
}
#[repr(C)]
pub struct VqAvail {
    pub flags: u16,
    pub idx: u16,
    pub ring: [u16; VIRTQ_SIZE],
    pub used_event: u16,
}
#[repr(C)]
pub struct VqUsedElem {
    pub idx: u32,
    pub len: u32,
}
#[repr(C)]
pub struct VqUsed {
    pub flags: u16,
    pub idx: u16,
    pub ring: [VqUsedElem; VIRTQ_SIZE],
    pub avail_event: u16,
}
#[repr(C, packed)]
pub struct BlkReq {
    pub req_type: u32,
    pub reserved: u32,
    pub sector: u64,
    pub data: [u8; VIRTIO_BLK_SECTOR],
    pub status: u8,
}

#[derive(Clone, Copy)]
pub struct VirtioBlkDev {
    pub base: usize,
    pub modern: bool,
    pub version: u32,
    pub desc: *mut VqDesc,
    pub avail: *mut VqAvail,
    pub used: *mut VqUsed,
    pub last_used: u16,
    pub req_buf: *mut BlkReq,
}

pub(crate) static mut G_DEVS: [VirtioBlkDev; VIRTIO_MAX_DEVS] = [VirtioBlkDev {
    base: 0,
    modern: false,
    version: 0,
    desc: ptr::null_mut(),
    avail: ptr::null_mut(),
    used: ptr::null_mut(),
    last_used: 0,
    req_buf: ptr::null_mut(),
}; VIRTIO_MAX_DEVS];
pub(crate) static mut G_NDEVS: usize = 0;

/// # Safety
///
/// `base` must be a virtio-mmio device base from the FDT probe or the fixed
/// QEMU virt constants (identity-mapped at boot); `off` a spec register offset.
#[inline]
pub(crate) unsafe fn reg_w(base: usize, off: u32, v: u32) {
    // SAFETY: base is a probed virtio-mmio base (FDT node / QEMU virt const) and off a spec register offset.
    unsafe {
        Mmio::<u32>::at(base + off as usize).write(v);
    }
}
/// # Safety
///
/// `base` must be a virtio-mmio device base from the FDT probe or the fixed
/// QEMU virt constants (identity-mapped at boot); `off` a spec register offset.
#[inline]
pub(crate) unsafe fn reg_r(base: usize, off: u32) -> u32 {
    // SAFETY: base is a probed virtio-mmio base (FDT node / QEMU virt const) and off a spec register offset.
    unsafe { Mmio::<u32>::at(base + off as usize).read() }
}

pub fn count() -> usize {
    // SAFETY: plain usize read of boot-probe state; kernel code never runs with SIE set (see crate::sync).
    unsafe { G_NDEVS }
}

/// # Safety
///
/// `idx` need not be valid: out-of-range indices yield a null pointer.
/// Callers must only dereference the result for a device fully initialized
/// by `init`, and only from kernel context (SIE=0, see crate::sync).
pub unsafe fn dev(idx: usize) -> *mut VirtioBlkDev {
    // SAFETY: G_NDEVS/G_DEVS written only during single-threaded boot probe; idx is bounds-checked against G_NDEVS and out-of-range values return null without touching the array.
    unsafe {
        let pn = &raw const G_NDEVS;
        if idx < *pn {
            let pd = &raw mut G_DEVS;
            &mut (*pd)[idx]
        } else {
            ptr::null_mut()
        }
    }
}

pub mod queue;
pub mod virtio_req;
pub mod virtio_rng;

pub use queue::{init, probe};

pub mod test;

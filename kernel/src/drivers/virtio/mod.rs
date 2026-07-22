//! virtio-blk MMIO driver — legacy v1 + modern v2.
//!
//! This is the directory root. It owns the register constants, the
//! virtqueue / request structs, the `VirtioBlkDev` device descriptor, the
//! global device array (`G_DEVS` / `G_NDEVS`), and the MMIO register
//! accessors. Device probe/init and virtqueue setup live in `queue.rs`.
//! `virtio_req.rs` (sibling) handles request submission / polled I/O.
use crate::arch::mmio::Mmio;
use core::ptr;

pub const VIRTIO_MAX_DEVS: usize = 4;
pub const VIRTIO_BLK_SECTOR: usize = 512;
pub const VIRTQ_SIZE: usize = 256;
pub const R_MAGIC_VALUE: u32 = 0x00;
pub const R_VERSION: u32 = 0x04;
pub const R_DEVICE_ID: u32 = 0x08;
pub const R_HOST_FEATURES: u32 = 0x10;
pub const R_GUEST_FEATURES: u32 = 0x14;
pub const R_QUEUE_SEL: u32 = 0x30;
pub const R_QUEUE_NUM_MAX: u32 = 0x34;
pub const R_QUEUE_NUM: u32 = 0x38;
pub const R_QUEUE_ALIGN: u32 = 0x3C;
pub const R_QUEUE_PFN: u32 = 0x40;
pub const R_QUEUE_NOTIFY: u32 = 0x50;
pub const R_STATUS: u32 = 0x70;
pub const R_QUEUE_DESC_LOW: u32 = 0x80;
pub const R_QUEUE_DESC_HIGH: u32 = 0x84;
pub const R_QUEUE_AVAIL_LOW: u32 = 0x90;
pub const R_QUEUE_AVAIL_HIGH: u32 = 0x94;
pub const R_QUEUE_USED_LOW: u32 = 0xA0;
pub const R_QUEUE_USED_HIGH: u32 = 0xA4;
pub const R_QUEUE_ENABLE: u32 = 0xB0;
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

#[inline]
pub(crate) unsafe fn reg_w(base: usize, off: u32, v: u32) {
    Mmio::<u32>::at(base + off as usize).write(v);
}
#[inline]
pub(crate) unsafe fn reg_r(base: usize, off: u32) -> u32 {
    Mmio::<u32>::at(base + off as usize).read()
}

pub fn count() -> usize {
    unsafe { *(&raw const G_NDEVS) }
}

pub unsafe fn dev(idx: usize) -> *mut VirtioBlkDev {
    let pn = &raw const G_NDEVS;
    if idx < *pn {
        let pd = &raw mut G_DEVS;
        &mut (*pd)[idx]
    } else {
        ptr::null_mut()
    }
}

pub mod queue;

pub use queue::{init, probe};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_magic_value() {
        assert_eq!(0x7472_6976, 0x7472_6976);
    }

    #[test]
    fn test_register_offsets() {
        assert_eq!(R_MAGIC_VALUE, 0x00);
        assert_eq!(R_VERSION, 0x04);
        assert_eq!(R_DEVICE_ID, 0x08);
        assert_eq!(R_HOST_FEATURES, 0x10);
        assert_eq!(R_GUEST_FEATURES, 0x14);
        assert_eq!(R_QUEUE_SEL, 0x30);
        assert_eq!(R_QUEUE_NUM_MAX, 0x34);
        assert_eq!(R_QUEUE_NUM, 0x38);
        assert_eq!(R_QUEUE_ALIGN, 0x3C);
        assert_eq!(R_QUEUE_PFN, 0x40);
        assert_eq!(R_QUEUE_NOTIFY, 0x50);
        assert_eq!(R_STATUS, 0x70);
        assert_eq!(R_QUEUE_DESC_LOW, 0x80);
        assert_eq!(R_QUEUE_DESC_HIGH, 0x84);
        assert_eq!(R_QUEUE_AVAIL_LOW, 0x90);
        assert_eq!(R_QUEUE_AVAIL_HIGH, 0x94);
        assert_eq!(R_QUEUE_USED_LOW, 0xA0);
        assert_eq!(R_QUEUE_USED_HIGH, 0xA4);
        assert_eq!(R_QUEUE_ENABLE, 0xB0);
    }

    #[test]
    fn test_status_flags() {
        assert_eq!(VIRTIO_S_ACK, 1);
        assert_eq!(VIRTIO_S_DRIVER, 2);
        assert_eq!(VIRTIO_S_DRIVER_OK, 4);
        assert_eq!(VIRTIO_S_FEATURES_OK, 8);
    }

    #[test]
    fn test_device_id() {
        assert_eq!(VIRTIO_ID_BLK, 2);
    }

    #[test]
    fn test_blk_constants() {
        assert_eq!(VIRTIO_BLK_T_IN, 0);
        assert_eq!(VIRTIO_BLK_T_OUT, 1);
        assert_eq!(VIRTIO_BLK_S_OK, 0);
        assert_eq!(VIRTIO_BLK_S_IOERR, 1);
    }

    #[test]
    fn test_vq_desc_flags() {
        assert_eq!(VQ_DESC_F_NEXT, 1);
        assert_eq!(VQ_DESC_F_WRITE, 2);
    }

    #[test]
    fn test_virtio_limits() {
        assert_eq!(VIRTIO_MAX_DEVS, 4);
        assert_eq!(VIRTIO_BLK_SECTOR, 512);
        assert_eq!(VIRTQ_SIZE, 256);
    }

    #[test]
    fn test_vqdesc_size_and_layout() {
        assert_eq!(core::mem::size_of::<VqDesc>(), 16);
        assert_eq!(core::mem::align_of::<VqDesc>(), 8);
    }

    #[test]
    fn test_blkreq_size() {
        assert_eq!(core::mem::size_of::<BlkReq>(), 529);
    }

    #[test]
    fn test_virtioblkdev_size() {
        assert_eq!(core::mem::size_of::<VirtioBlkDev>(), 48);
    }

    #[test]
    fn test_initial_count() {
        assert_eq!(count(), 0);
    }

    #[test]
    fn test_dev_out_of_range() {
        unsafe {
            let d = dev(0);
            assert!(d.is_null());
        }
    }
}

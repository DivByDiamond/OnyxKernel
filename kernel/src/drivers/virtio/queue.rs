//! virtio-blk device probe / init and virtqueue setup.
use super::{
    BlkReq, G_DEVS, G_NDEVS, R_DEVICE_ID, R_GUEST_FEATURES, R_GUEST_FEATURES_SEL,
    R_GUEST_PAGE_SIZE, R_HOST_FEATURES, R_HOST_FEATURES_SEL, R_MAGIC_VALUE, R_QUEUE_ALIGN,
    R_QUEUE_AVAIL_HIGH, R_QUEUE_AVAIL_LOW, R_QUEUE_DESC_HIGH, R_QUEUE_DESC_LOW, R_QUEUE_NUM,
    R_QUEUE_PFN, R_QUEUE_READY, R_QUEUE_SEL, R_QUEUE_USED_HIGH, R_QUEUE_USED_LOW, R_STATUS,
    R_VERSION, VIRTIO_F_VERSION_1, VIRTIO_ID_BLK, VIRTIO_MAX_DEVS, VIRTIO_S_ACK, VIRTIO_S_DRIVER,
    VIRTIO_S_DRIVER_OK, VIRTIO_S_FEATURES_OK, VIRTQ_SIZE, VirtioBlkDev, VqAvail, VqDesc, VqUsed,
    reg_r, reg_w,
};
use crate::mm::pmm;
use core::ptr;
use onyx_core::errno::{Errno, KResult};

/// # Safety
///
/// `base` must be a candidate virtio-mmio base from the FDT probe or the
/// QEMU virt fallback constants (identity-mapped at boot).
pub unsafe fn probe(base: usize) -> bool {
    // SAFETY: base is a candidate virtio-mmio base from the boot-time probe; reg_r reads only spec offsets (magic, device ID).
    unsafe {
        let magic = reg_r(base, R_MAGIC_VALUE);
        if magic != 0x7472_6976 {
            return false;
        }
        reg_r(base, R_DEVICE_ID) == VIRTIO_ID_BLK
    }
}

/// # Safety
///
/// `base` must be a probed virtio-blk MMIO base; must be called during the
/// single-threaded boot-time device probe, once per base.
pub unsafe fn init(base: usize) -> KResult<usize> {
    // SAFETY: boot-time single-threaded probe on a probed base; G_DEVS/G_NDEVS touched only here (SIE=0, see crate::sync), slot bounded by VIRTIO_MAX_DEVS.
    unsafe {
        let pn = &raw const G_NDEVS;
        if *pn >= VIRTIO_MAX_DEVS {
            return Err(Errno::NoMem);
        }
        let idx = *pn;
        let version = reg_r(base, R_VERSION);
        let modern = version >= 2;
        let dev = VirtioBlkDev {
            base,
            modern,
            version,
            desc: ptr::null_mut(),
            avail: ptr::null_mut(),
            used: ptr::null_mut(),
            last_used: 0,
            req_buf: ptr::null_mut(),
        };
        G_DEVS[idx] = dev;
        reg_w(base, R_STATUS, 0);
        reg_w(base, R_STATUS, VIRTIO_S_ACK | VIRTIO_S_DRIVER);
        // Read the full 64-bit feature word via the features_sel registers.
        reg_w(base, R_HOST_FEATURES_SEL, 1);
        let host_feat_hi = reg_r(base, R_HOST_FEATURES);
        reg_w(base, R_HOST_FEATURES_SEL, 0);
        let host_feat_lo = reg_r(base, R_HOST_FEATURES);
        let mut guest_hi = host_feat_hi;
        guest_hi |= VIRTIO_F_VERSION_1; // bit 32: must accept VIRTIO_F_VERSION_1.
        reg_w(base, R_GUEST_FEATURES_SEL, 0);
        reg_w(base, R_GUEST_FEATURES, host_feat_lo & 0x1FFF_FFFF);
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
                // sedna's virtio emulation never sets FEATURES_OK, yet it still
                // accepts DRIVER_OK — don't fail the whole device over it.
                crate::kwrn!("virtio", "device did not set FEATURES_OK, continuing");
            }
        }
        setup_queue(idx)?;
        reg_w(
            base,
            R_STATUS,
            VIRTIO_S_ACK | VIRTIO_S_DRIVER | VIRTIO_S_FEATURES_OK | VIRTIO_S_DRIVER_OK,
        );
        G_NDEVS += 1;
        Ok(idx)
    }
}

/// # Safety
///
/// `idx` must be the slot just stored by `init` (so `idx < G_NDEVS`) and the
/// call must happen during single-threaded boot-time probe.
unsafe fn setup_queue(idx: usize) -> KResult<()> {
    // SAFETY: idx < G_NDEVS guaranteed by init; rings/req buffer are fresh contiguous PMM pages stored into the device before it is told the addresses.
    unsafe {
        let pd = &raw mut G_DEVS;
        let dev = &mut (*pd)[idx];
        reg_w(dev.base, R_QUEUE_SEL, 0);
        reg_w(dev.base, R_QUEUE_NUM, VIRTQ_SIZE as u32);
        if dev.modern {
            let desc_pa = pmm::alloc_zero()? as usize;
            let avail_pa = pmm::alloc_zero()? as usize;
            let used_pa = pmm::alloc_zero()? as usize;
            let req_pa = pmm::alloc_zero()? as usize;
            dev.desc = desc_pa as *mut VqDesc;
            dev.avail = avail_pa as *mut VqAvail;
            dev.used = used_pa as *mut VqUsed;
            dev.req_buf = req_pa as *mut BlkReq;
            dev.last_used = 0;
            reg_w(dev.base, R_QUEUE_DESC_LOW, desc_pa as u32);
            reg_w(dev.base, R_QUEUE_DESC_HIGH, ((desc_pa as u64) >> 32) as u32);
            reg_w(dev.base, R_QUEUE_AVAIL_LOW, avail_pa as u32);
            reg_w(
                dev.base,
                R_QUEUE_AVAIL_HIGH,
                ((avail_pa as u64) >> 32) as u32,
            );
            reg_w(dev.base, R_QUEUE_USED_LOW, used_pa as u32);
            reg_w(dev.base, R_QUEUE_USED_HIGH, ((used_pa as u64) >> 32) as u32);
            reg_w(dev.base, R_QUEUE_READY, 1);
        } else {
            let contig_pa = pmm::alloc_n(3)? as usize;
            let desc_pa = contig_pa;
            let avail_pa = contig_pa + 4096;
            let used_pa = contig_pa + 2 * 4096;
            let req_pa = pmm::alloc_zero()? as usize;
            dev.desc = desc_pa as *mut VqDesc;
            dev.avail = avail_pa as *mut VqAvail;
            dev.used = used_pa as *mut VqUsed;
            dev.req_buf = req_pa as *mut BlkReq;
            dev.last_used = 0;
            reg_w(dev.base, R_GUEST_PAGE_SIZE, 4096);
            reg_w(dev.base, R_QUEUE_ALIGN, 4096);
            reg_w(dev.base, R_QUEUE_PFN, (desc_pa / 4096) as u32);
        }
        Ok(())
    }
}

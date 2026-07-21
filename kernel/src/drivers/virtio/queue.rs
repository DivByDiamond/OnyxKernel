//! virtio-blk device probe / init and virtqueue setup.
use super::{
    reg_r, reg_w, BlkReq, VirtioBlkDev, VqAvail, VqDesc, VqUsed, G_DEVS, G_NDEVS, R_DEVICE_ID,
    R_GUEST_FEATURES, R_HOST_FEATURES, R_MAGIC_VALUE, R_QUEUE_ALIGN, R_QUEUE_AVAIL_HIGH,
    R_QUEUE_AVAIL_LOW, R_QUEUE_DESC_HIGH, R_QUEUE_DESC_LOW, R_QUEUE_ENABLE, R_QUEUE_NUM,
    R_QUEUE_PFN, R_QUEUE_SEL, R_QUEUE_USED_HIGH, R_QUEUE_USED_LOW, R_STATUS, R_VERSION,
    VIRTIO_ID_BLK, VIRTIO_MAX_DEVS, VIRTIO_S_ACK, VIRTIO_S_DRIVER, VIRTIO_S_DRIVER_OK,
    VIRTIO_S_FEATURES_OK, VIRTQ_SIZE,
};
use crate::mm::pmm;
use core::ptr;
use onyx_core::errno::{Errno, KResult};

pub unsafe fn probe(base: usize) -> bool {
    let magic = reg_r(base, R_MAGIC_VALUE);
    if magic != 0x7472_6976 {
        return false;
    }
    reg_r(base, R_DEVICE_ID) == VIRTIO_ID_BLK
}

pub unsafe fn init(base: usize) -> KResult<usize> {
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
    (*(&raw mut G_DEVS))[idx] = dev;
    reg_w(base, R_STATUS, 0);
    reg_w(base, R_STATUS, VIRTIO_S_ACK | VIRTIO_S_DRIVER);
    let host_feat = reg_r(base, R_HOST_FEATURES);
    reg_w(base, R_GUEST_FEATURES, host_feat & 0x1FFF_FFFF);
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
    setup_queue(idx)?;
    reg_w(
        base,
        R_STATUS,
        VIRTIO_S_ACK | VIRTIO_S_DRIVER | VIRTIO_S_DRIVER_OK,
    );
    *(&raw mut G_NDEVS) += 1;
    Ok(idx)
}

unsafe fn setup_queue(idx: usize) -> KResult<()> {
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
        reg_w(dev.base, R_QUEUE_ENABLE, 1);
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
        reg_w(dev.base, R_QUEUE_ALIGN, 4096);
        reg_w(dev.base, R_QUEUE_PFN, (desc_pa / 4096) as u32);
    }
    Ok(())
}

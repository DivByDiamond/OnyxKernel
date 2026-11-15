//! Virtio-input device bring-up: feature negotiation, queue setup and the
//! initial descriptor posting.

use super::{InDev, N_EVENTS, VirtioInputEvent};
use crate::drivers::virtio::{
    R_GUEST_FEATURES, R_GUEST_FEATURES_SEL, R_HOST_FEATURES, R_HOST_FEATURES_SEL,
    R_QUEUE_AVAIL_HIGH, R_QUEUE_AVAIL_LOW, R_QUEUE_DESC_HIGH, R_QUEUE_DESC_LOW, R_QUEUE_NUM,
    R_QUEUE_READY, R_QUEUE_SEL, R_QUEUE_USED_HIGH, R_QUEUE_USED_LOW, R_STATUS, R_VERSION,
    VIRTIO_F_VERSION_1, VIRTIO_S_ACK, VIRTIO_S_DRIVER, VIRTIO_S_DRIVER_OK, VIRTIO_S_FEATURES_OK,
    VIRTQ_SIZE, VQ_DESC_F_WRITE, VqAvail, VqDesc, VqUsed, reg_r, reg_w,
};
use crate::mm::pmm;
use core::ptr;
use onyx_core::errno::{Errno, KResult};

pub(super) unsafe fn init_device(base: usize, dev: &mut InDev) -> KResult<()> {
    unsafe {
        let version = reg_r(base, R_VERSION);
        let modern = version >= 2;
        dev.base = base;
        dev.modern = modern;
        reg_w(base, R_STATUS, 0);
        reg_w(base, R_STATUS, VIRTIO_S_ACK | VIRTIO_S_DRIVER);
        reg_w(base, R_HOST_FEATURES_SEL, 0);
        let hf = reg_r(base, R_HOST_FEATURES);
        reg_w(base, R_GUEST_FEATURES_SEL, 0);
        reg_w(base, R_GUEST_FEATURES, hf & 0x1FFF_FFFF);
        if modern {
            reg_w(base, R_HOST_FEATURES_SEL, 1);
            let high = reg_r(base, R_HOST_FEATURES);
            reg_w(base, R_HOST_FEATURES_SEL, 0);
            if high & VIRTIO_F_VERSION_1 == 0 {
                return Err(Errno::Inval);
            }
            reg_w(base, R_GUEST_FEATURES_SEL, 1);
            reg_w(base, R_GUEST_FEATURES, VIRTIO_F_VERSION_1);
            reg_w(base, R_GUEST_FEATURES_SEL, 0);
        }
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
        setup_event_queue(dev)?;
        reg_w(
            base,
            R_STATUS,
            VIRTIO_S_ACK
                | VIRTIO_S_DRIVER
                | VIRTIO_S_DRIVER_OK
                | if modern { VIRTIO_S_FEATURES_OK } else { 0 },
        );
        dev.ready = true;
        Ok(())
    }
}

/// # Safety
///
/// Must be called from `init` with dev.base set to a probed base, during
/// single-threaded boot-time probe.
unsafe fn setup_event_queue(dev: &mut InDev) -> KResult<()> {
    // SAFETY: called from init on a probed base during single-threaded boot; rings and the N_EVENTS-entry buffer are fresh contiguous PMM pages stored into G_IN and registered before use; desc slots 0..N_EVENTS pre-posted, N_EVENTS <= VIRTQ_SIZE.
    unsafe {
        let base = dev.base;
        reg_w(base, R_QUEUE_SEL, 0);
        reg_w(base, R_QUEUE_NUM, VIRTQ_SIZE as u32);
        dev.desc = pmm::alloc_zero()? as *mut VqDesc;
        dev.avail = pmm::alloc_zero()? as *mut VqAvail;
        dev.used = pmm::alloc_zero()? as *mut VqUsed;
        dev.ev_buf = pmm::alloc_zero()? as *mut VirtioInputEvent;
        let desc_pa = dev.desc as usize;
        let avail_pa = dev.avail as usize;
        let used_pa = dev.used as usize;
        let ev_pa = dev.ev_buf;
        reg_w(base, R_QUEUE_DESC_LOW, desc_pa as u32);
        reg_w(base, R_QUEUE_DESC_HIGH, ((desc_pa as u64) >> 32) as u32);
        reg_w(base, R_QUEUE_AVAIL_LOW, avail_pa as u32);
        reg_w(base, R_QUEUE_AVAIL_HIGH, ((avail_pa as u64) >> 32) as u32);
        reg_w(base, R_QUEUE_USED_LOW, used_pa as u32);
        reg_w(base, R_QUEUE_USED_HIGH, ((used_pa as u64) >> 32) as u32);
        reg_w(base, R_QUEUE_READY, 1);
        // Pre-post N_EVENTS descriptors for incoming events.
        for i in 0..N_EVENTS {
            let off = (ev_pa as usize) + i * core::mem::size_of::<VirtioInputEvent>();
            (*dev.desc.add(i)) = VqDesc {
                addr: off as u64,
                len: core::mem::size_of::<VirtioInputEvent>() as u32,
                flags: VQ_DESC_F_WRITE,
                next: 0,
            };
            push(dev, i);
        }
        Ok(())
    }
}

/// # Safety
///
/// dev.avail must have been initialized by `init` (non-null); `idx` must
/// be a valid descriptor index (< N_EVENTS).
pub(super) unsafe fn push(dev: &mut InDev, idx: usize) {
    // SAFETY: dev.avail is a PMM ring set up by init; ring slot masked % VIRTQ_SIZE per spec; volatile write + SeqCst fence order entry before idx bump.
    unsafe {
        let i = ptr::read_volatile(ptr::addr_of!((*dev.avail).idx));
        ptr::write_volatile(
            ptr::addr_of_mut!((*dev.avail).ring[(i as usize) % VIRTQ_SIZE]),
            idx as u16,
        );
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        ptr::write_volatile(ptr::addr_of_mut!((*dev.avail).idx), i.wrapping_add(1));
    }
}

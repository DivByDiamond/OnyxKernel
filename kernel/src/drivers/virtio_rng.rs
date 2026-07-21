//! virtio-rng MMIO driver — entropy source.
//!
//! QEMU virt exposes a virtio-rng device when invoked with
//! `-device virtio-rng-device`. The driver reuses the existing virtio
//! MMIO constants and provides a single `read()` entry point.
use crate::drivers::virtio::{
    reg_r, reg_w, VqAvail, VqDesc, VqUsed, R_DEVICE_ID, R_GUEST_FEATURES, R_HOST_FEATURES,
    R_MAGIC_VALUE, R_QUEUE_ALIGN, R_QUEUE_AVAIL_HIGH, R_QUEUE_AVAIL_LOW, R_QUEUE_DESC_HIGH,
    R_QUEUE_DESC_LOW, R_QUEUE_ENABLE, R_QUEUE_NUM, R_QUEUE_PFN, R_QUEUE_SEL, R_QUEUE_USED_HIGH,
    R_QUEUE_USED_LOW, R_STATUS, R_VERSION, VIRTIO_S_ACK, VIRTIO_S_DRIVER, VIRTIO_S_DRIVER_OK,
    VIRTIO_S_FEATURES_OK, VIRTQ_SIZE, VQ_DESC_F_WRITE,
};
use crate::mm::pmm;
use core::ptr;
use onyx_core::errno::{Errno, KResult};

const VIRTIO_ID_RNG: u32 = 4;
const MAX_RNG_DEVS: usize = 2;

#[derive(Clone, Copy)]
struct RngDev {
    base: usize,
    modern: bool,
    desc: *mut VqDesc,
    avail: *mut VqAvail,
    used: *mut VqUsed,
    last_used: u16,
}

static mut G_DEVS_RNG: [RngDev; MAX_RNG_DEVS] = [RngDev {
    base: 0,
    modern: false,
    desc: ptr::null_mut(),
    avail: ptr::null_mut(),
    used: ptr::null_mut(),
    last_used: 0,
}; MAX_RNG_DEVS];
static mut G_N: usize = 0;

pub fn is_present() -> bool {
    unsafe { G_N > 0 }
}

pub unsafe fn probe(base: usize) -> bool {
    let magic = reg_r(base, R_MAGIC_VALUE);
    if magic != 0x7472_6976 {
        return false;
    }
    reg_r(base, R_DEVICE_ID) == VIRTIO_ID_RNG
}

pub unsafe fn init(base: usize) -> KResult<()> {
    let pn = &raw const G_N;
    if *pn >= MAX_RNG_DEVS {
        return Err(Errno::NoMem);
    }
    let idx = *pn;
    let version = reg_r(base, R_VERSION);
    let modern = version >= 2;
    let dev = RngDev {
        base,
        modern,
        desc: ptr::null_mut(),
        avail: ptr::null_mut(),
        used: ptr::null_mut(),
        last_used: 0,
    };
    (*(&raw mut G_DEVS_RNG))[idx] = dev;
    reg_w(base, R_STATUS, 0);
    reg_w(base, R_STATUS, VIRTIO_S_ACK | VIRTIO_S_DRIVER);
    let hf = reg_r(base, R_HOST_FEATURES);
    reg_w(base, R_GUEST_FEATURES, hf & 0x1FFF_FFFF);
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
    *(&raw mut G_N) += 1;
    Ok(())
}

unsafe fn setup_queue(idx: usize) -> KResult<()> {
    let p = &raw mut G_DEVS_RNG;
    let dev = &mut (*p)[idx];
    reg_w(dev.base, R_QUEUE_SEL, 0);
    reg_w(dev.base, R_QUEUE_NUM, VIRTQ_SIZE as u32);
    if dev.modern {
        let desc_pa = pmm::alloc_zero()? as usize;
        let avail_pa = pmm::alloc_zero()? as usize;
        let used_pa = pmm::alloc_zero()? as usize;
        dev.desc = desc_pa as *mut VqDesc;
        dev.avail = avail_pa as *mut VqAvail;
        dev.used = used_pa as *mut VqUsed;
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
        let pa = pmm::alloc_n(3)? as usize;
        dev.desc = pa as *mut VqDesc;
        dev.avail = (pa + 4096) as *mut VqAvail;
        dev.used = (pa + 8192) as *mut VqUsed;
        reg_w(dev.base, R_QUEUE_ALIGN, 4096);
        reg_w(dev.base, R_QUEUE_PFN, (pa / 4096) as u32);
    }
    Ok(())
}

/// Synchronously read `buf.len()` bytes of entropy. `buf` must be 4K-aligned
/// and at most one page; the caller should chunk larger requests.
pub fn read(buf: &mut [u8]) -> KResult<()> {
    if buf.is_empty() || buf.len() > 4096 {
        return Err(Errno::Inval);
    }
    unsafe {
        let p = &raw mut G_DEVS_RNG;
        let dev = &mut (*p)[0];
        if dev.base == 0 {
            return Err(Errno::NoEnt);
        }
        // Submit one descriptor: writable, points at `buf`.
        let pa = buf.as_ptr() as u64;
        (*dev.desc.add(0)) = VqDesc {
            addr: pa,
            len: buf.len() as u32,
            flags: VQ_DESC_F_WRITE,
            next: 0,
        };
        let idx = ptr::read_volatile(ptr::addr_of!((*dev.avail).idx));
        ptr::write_volatile(
            ptr::addr_of_mut!((*dev.avail).ring[(idx as usize) % VIRTQ_SIZE]),
            0,
        );
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        ptr::write_volatile(ptr::addr_of_mut!((*dev.avail).idx), idx.wrapping_add(1));
        reg_w(dev.base, crate::drivers::virtio::R_QUEUE_NOTIFY, 0);
        let used_idx_ptr = ptr::addr_of!((*dev.used).idx);
        #[allow(clippy::while_immutable_condition)]
        while ptr::read_volatile(used_idx_ptr) == dev.last_used {}
        dev.last_used = ptr::read_volatile(used_idx_ptr);
        Ok(())
    }
}

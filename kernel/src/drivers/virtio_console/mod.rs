//! Polled virtio-console byte stream, exposed as /dev/hvc0 for OC2R RPC.
use crate::drivers::virtio::{
    R_DEVICE_ID, R_GUEST_FEATURES, R_GUEST_FEATURES_SEL, R_GUEST_PAGE_SIZE,
    R_HOST_FEATURES, R_HOST_FEATURES_SEL, R_MAGIC_VALUE, R_QUEUE_ALIGN,
    R_QUEUE_AVAIL_HIGH, R_QUEUE_AVAIL_LOW, R_QUEUE_DESC_HIGH, R_QUEUE_DESC_LOW,
    R_QUEUE_NUM, R_QUEUE_NUM_MAX, R_QUEUE_PFN, R_QUEUE_READY, R_QUEUE_SEL,
    R_QUEUE_USED_HIGH, R_QUEUE_USED_LOW, R_STATUS, R_VERSION, VIRTIO_F_VERSION_1,
    VIRTIO_S_ACK, VIRTIO_S_DRIVER, VIRTIO_S_DRIVER_OK, VIRTIO_S_FEATURES_OK,
    VQ_DESC_F_WRITE, VqAvail, VqDesc, VqUsed, reg_r, reg_w,
};
use crate::{mm::pmm, sync::SpinLock};
use core::ptr;
use onyx_core::errno::{Errno, KResult};

pub const VIRTIO_ID_CONSOLE: u32 = 3;
pub const BUF_SIZE: usize = 4096;
pub(crate) const QUEUE_SIZE: usize = 4; // Sedna console advertises a maximum of four.
pub(crate) static LOCK: SpinLock = SpinLock::new();

pub(crate) struct ConDev {
    base: usize,
    rx_desc: *mut VqDesc,
    rx_avail: *mut VqAvail,
    rx_used: *mut VqUsed,
    tx_desc: *mut VqDesc,
    tx_avail: *mut VqAvail,
    tx_used: *mut VqUsed,
    rx_last: u16,
    rx_offset: usize,
    rx_len: usize,
    tx_last: u16,
    tx_busy: bool,
    rx_buf: *mut u8,
    tx_buf: *mut u8,
}

pub(crate) static mut G_CON: ConDev = ConDev {
    base: 0, rx_desc: ptr::null_mut(), rx_avail: ptr::null_mut(),
    rx_used: ptr::null_mut(), tx_desc: ptr::null_mut(), tx_avail: ptr::null_mut(),
    tx_used: ptr::null_mut(), rx_last: 0, rx_offset: 0, rx_len: 0,
    tx_last: 0, tx_busy: false, rx_buf: ptr::null_mut(), tx_buf: ptr::null_mut(),
};

/// # Safety
/// `base` is a mapped virtio MMIO candidate discovered at boot.
pub unsafe fn probe(base: usize) -> bool {
    unsafe { reg_r(base, R_MAGIC_VALUE) == 0x7472_6976 && reg_r(base, R_DEVICE_ID) == VIRTIO_ID_CONSOLE }
}

/// # Safety
/// Single-threaded boot, mapped and probed console device.
pub unsafe fn init(base: usize) -> KResult<()> {
    unsafe {
        if G_CON.base != 0 { return Err(Errno::Busy); }
        let modern = reg_r(base, R_VERSION) >= 2;
        reg_w(base, R_STATUS, 0);
        reg_w(base, R_STATUS, VIRTIO_S_ACK | VIRTIO_S_DRIVER);
        // Only negotiate features we implement. In particular MULTIPORT is
        // deliberately absent: queues 0 and 1 remain the single console port.
        reg_w(base, R_GUEST_FEATURES_SEL, 0);
        reg_w(base, R_GUEST_FEATURES, 0);
        if modern {
            reg_w(base, R_HOST_FEATURES_SEL, 1);
            if reg_r(base, R_HOST_FEATURES) & VIRTIO_F_VERSION_1 == 0 { return Err(Errno::Inval); }
            reg_w(base, R_HOST_FEATURES_SEL, 0);
            reg_w(base, R_GUEST_FEATURES_SEL, 1);
            reg_w(base, R_GUEST_FEATURES, VIRTIO_F_VERSION_1);
            reg_w(base, R_GUEST_FEATURES_SEL, 0);
            reg_w(base, R_STATUS, VIRTIO_S_ACK | VIRTIO_S_DRIVER | VIRTIO_S_FEATURES_OK);
            if reg_r(base, R_STATUS) & VIRTIO_S_FEATURES_OK == 0 { return Err(Errno::Inval); }
        }
        setup_queue(base, modern, 0)?;
        setup_queue(base, modern, 1)?;
        G_CON.rx_buf = pmm::alloc_zero()? as *mut u8;
        G_CON.tx_buf = pmm::alloc_zero()? as *mut u8;
        *G_CON.rx_desc = VqDesc { addr: G_CON.rx_buf as u64, len: BUF_SIZE as u32, flags: VQ_DESC_F_WRITE, next: 0 };
        push(0, true);
        let features = if modern { VIRTIO_S_FEATURES_OK } else { 0 };
        reg_w(base, R_STATUS, VIRTIO_S_ACK | VIRTIO_S_DRIVER | features | VIRTIO_S_DRIVER_OK);
        G_CON.base = base; // Publish only after all rings and buffers exist.
        reg_w(base, crate::drivers::virtio::R_QUEUE_NOTIFY, 0);
        Ok(())
    }
}

unsafe fn setup_queue(base: usize, modern: bool, index: u32) -> KResult<()> {
    unsafe {
        reg_w(base, R_QUEUE_SEL, index);
        if reg_r(base, R_QUEUE_NUM_MAX) < QUEUE_SIZE as u32 { return Err(Errno::Inval); }
        reg_w(base, R_QUEUE_NUM, QUEUE_SIZE as u32);
        let (desc, avail, used) = if modern {
            let d = pmm::alloc_zero()? as usize;
            let a = pmm::alloc_zero()? as usize;
            let u = pmm::alloc_zero()? as usize;
            reg_w(base, R_QUEUE_DESC_LOW, d as u32); reg_w(base, R_QUEUE_DESC_HIGH, (d as u64 >> 32) as u32);
            reg_w(base, R_QUEUE_AVAIL_LOW, a as u32); reg_w(base, R_QUEUE_AVAIL_HIGH, (a as u64 >> 32) as u32);
            reg_w(base, R_QUEUE_USED_LOW, u as u32); reg_w(base, R_QUEUE_USED_HIGH, (u as u64 >> 32) as u32);
            reg_w(base, R_QUEUE_READY, 1);
            (d, a, u)
        } else {
            let d = pmm::alloc_n(3)? as usize;
            reg_w(base, R_GUEST_PAGE_SIZE, 4096); reg_w(base, R_QUEUE_ALIGN, 4096);
            reg_w(base, R_QUEUE_PFN, (d / 4096) as u32);
            (d, d + QUEUE_SIZE * core::mem::size_of::<VqDesc>(), d + 4096)
        };
        if index == 0 {
            G_CON.rx_desc = desc as *mut VqDesc; G_CON.rx_avail = avail as *mut VqAvail; G_CON.rx_used = used as *mut VqUsed;
        } else {
            G_CON.tx_desc = desc as *mut VqDesc; G_CON.tx_avail = avail as *mut VqAvail; G_CON.tx_used = used as *mut VqUsed;
        }
        Ok(())
    }
}

pub fn present() -> bool { unsafe { G_CON.base != 0 } }

/// Caller owns LOCK (or is the boot-time initializer).
pub(crate) unsafe fn push(index: usize, receive: bool) {
    unsafe {
        let avail = if receive { G_CON.rx_avail } else { G_CON.tx_avail };
        let next = ptr::read_volatile(ptr::addr_of!((*avail).idx));
        ptr::write_volatile(ptr::addr_of_mut!((*avail).ring[next as usize % QUEUE_SIZE]), index as u16);
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        ptr::write_volatile(ptr::addr_of_mut!((*avail).idx), next.wrapping_add(1));
    }
}

pub mod xfer;
pub use xfer::{getc, putc, puts, read, write};

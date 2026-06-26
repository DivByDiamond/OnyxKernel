//! virtio-console MMIO driver — root module.
//!
//! Owns the device struct, global state, and probe/init. RX/TX queue
//! helpers live in `xfer.rs`.
use crate::drivers::virtio::{
    reg_r, reg_w, VqAvail, VqDesc, VqUsed, R_DEVICE_ID, R_GUEST_FEATURES, R_HOST_FEATURES,
    R_MAGIC_VALUE, R_QUEUE_AVAIL_HIGH, R_QUEUE_AVAIL_LOW, R_QUEUE_DESC_HIGH, R_QUEUE_DESC_LOW,
    R_QUEUE_ENABLE, R_QUEUE_NUM, R_QUEUE_SEL, R_QUEUE_USED_HIGH, R_QUEUE_USED_LOW, R_STATUS,
    R_VERSION, VIRTIO_S_ACK, VIRTIO_S_DRIVER, VIRTIO_S_DRIVER_OK, VIRTIO_S_FEATURES_OK,
    VIRTQ_SIZE, VQ_DESC_F_WRITE,
};
use crate::mm::pmm;
use core::ptr;
use onyx_core::errno::{Errno, KResult};

pub const VIRTIO_ID_CONSOLE: u32 = 3;
pub const BUF_SIZE: usize = 256;

#[derive(Clone, Copy)]
pub(crate) struct ConDev {
    pub base: usize,
    pub modern: bool,
    pub rx_desc: *mut VqDesc,
    pub rx_avail: *mut VqAvail,
    pub rx_used: *mut VqUsed,
    pub tx_desc: *mut VqDesc,
    pub tx_avail: *mut VqAvail,
    pub tx_used: *mut VqUsed,
    pub rx_last: u16,
    pub _tx_last: u16,
    pub rx_buf: *mut u8,
}

pub(crate) static mut G_CON: ConDev = ConDev {
    base: 0,
    modern: false,
    rx_desc: ptr::null_mut(),
    rx_avail: ptr::null_mut(),
    rx_used: ptr::null_mut(),
    tx_desc: ptr::null_mut(),
    tx_avail: ptr::null_mut(),
    tx_used: ptr::null_mut(),
    rx_last: 0,
    _tx_last: 0,
    rx_buf: ptr::null_mut(),
};

pub unsafe fn probe(base: usize) -> bool {
    if reg_r(base, R_MAGIC_VALUE) != 0x7472_6976 {
        return false;
    }
    reg_r(base, R_DEVICE_ID) == VIRTIO_ID_CONSOLE
}

pub unsafe fn init(base: usize) -> KResult<()> {
    if G_CON.base != 0 {
        return Err(Errno::Busy);
    }
    let version = reg_r(base, R_VERSION);
    let modern = version >= 2;
    G_CON.base = base;
    G_CON.modern = modern;
    reg_w(base, R_STATUS, 0);
    reg_w(base, R_STATUS, VIRTIO_S_ACK | VIRTIO_S_DRIVER);
    let hf = reg_r(base, R_HOST_FEATURES);
    reg_w(base, R_GUEST_FEATURES, hf & 0x1FFF_FFFF);
    if modern {
        reg_w(base, R_STATUS,
            VIRTIO_S_ACK | VIRTIO_S_DRIVER | VIRTIO_S_FEATURES_OK);
        if reg_r(base, R_STATUS) & VIRTIO_S_FEATURES_OK == 0 {
            return Err(Errno::Inval);
        }
    }
    setup_queue(0, true)?;  // RX (queue 0)
    setup_queue(1, false)?; // TX (queue 1)
    reg_w(base, R_STATUS, VIRTIO_S_ACK | VIRTIO_S_DRIVER | VIRTIO_S_DRIVER_OK);
    Ok(())
}

unsafe fn setup_queue(qidx: u32, is_rx: bool) -> KResult<()> {
    let base = G_CON.base;
    reg_w(base, R_QUEUE_SEL, qidx);
    reg_w(base, R_QUEUE_NUM, VIRTQ_SIZE as u32);
    let desc_pa = pmm::alloc_zero()? as usize;
    let avail_pa = pmm::alloc_zero()? as usize;
    let used_pa = pmm::alloc_zero()? as usize;
    let buf_pa = pmm::alloc_zero()? as *mut u8;
    let desc = desc_pa as *mut VqDesc;
    let avail = avail_pa as *mut VqAvail;
    let used = used_pa as *mut VqUsed;
    reg_w(base, R_QUEUE_DESC_LOW, desc_pa as u32);
    reg_w(base, R_QUEUE_DESC_HIGH, ((desc_pa as u64) >> 32) as u32);
    reg_w(base, R_QUEUE_AVAIL_LOW, avail_pa as u32);
    reg_w(base, R_QUEUE_AVAIL_HIGH, ((avail_pa as u64) >> 32) as u32);
    reg_w(base, R_QUEUE_USED_LOW, used_pa as u32);
    reg_w(base, R_QUEUE_USED_HIGH, ((used_pa as u64) >> 32) as u32);
    reg_w(base, R_QUEUE_ENABLE, 1);
    if is_rx {
        G_CON.rx_desc = desc;
        G_CON.rx_avail = avail;
        G_CON.rx_used = used;
        G_CON.rx_buf = buf_pa;
        // Pre-post a single RX descriptor.
        (*desc.add(0)) = VqDesc {
            addr: buf_pa as u64,
            len: BUF_SIZE as u32,
            flags: VQ_DESC_F_WRITE,
            next: 0,
        };
        push(0, true);
    } else {
        G_CON.tx_desc = desc;
        G_CON.tx_avail = avail;
        G_CON.tx_used = used;
    }
    Ok(())
}

pub(crate) unsafe fn push(idx: usize, is_rx: bool) {
    let avail = if is_rx { G_CON.rx_avail } else { G_CON.tx_avail };
    let i = ptr::read_volatile(ptr::addr_of!((*avail).idx));
    ptr::write_volatile(
        ptr::addr_of_mut!((*avail).ring[(i as usize) % VIRTQ_SIZE]),
        idx as u16,
    );
    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
    ptr::write_volatile(ptr::addr_of_mut!((*avail).idx), i.wrapping_add(1));
}

pub mod xfer;
pub use xfer::{getc, puts, putc};

//! virtio-net MMIO driver — root module.
//!
//! Owns the device struct, global state, register constants, and the
//! probe / init sequence. Frame I/O lives in `xfer.rs`.
use crate::arch::mmio::Mmio;
use crate::drivers::virtio::{
    reg_r, reg_w, VqAvail, VqDesc, VqUsed, R_DEVICE_ID, R_GUEST_FEATURES, R_HOST_FEATURES,
    R_MAGIC_VALUE, R_QUEUE_AVAIL_HIGH, R_QUEUE_AVAIL_LOW, R_QUEUE_DESC_HIGH, R_QUEUE_DESC_LOW,
    R_QUEUE_ENABLE, R_QUEUE_NUM, R_QUEUE_SEL, R_QUEUE_USED_HIGH, R_QUEUE_USED_LOW, R_STATUS,
    R_VERSION, VIRTIO_S_ACK, VIRTIO_S_DRIVER, VIRTIO_S_DRIVER_OK, VIRTIO_S_FEATURES_OK, VIRTQ_SIZE,
    VQ_DESC_F_WRITE,
};
use crate::mm::pmm;
use core::ptr;
use onyx_core::errno::{Errno, KResult};

pub const VIRTIO_ID_NET: u32 = 1;
pub const NET_MTU: usize = 1514;
pub const RX_DESCS: usize = 16;
pub const HDR_LEN: usize = 12;

#[derive(Clone, Copy)]
pub(crate) struct NetDev {
    pub base: usize,
    pub modern: bool,
    pub desc: *mut VqDesc,
    pub avail: *mut VqAvail,
    pub used: *mut VqUsed,
    pub last_used: u16,
    pub rx_bufs: [*mut u8; RX_DESCS],
    pub mac: [u8; 6],
}

pub(crate) static mut G_NET: NetDev = NetDev {
    base: 0,
    modern: false,
    desc: ptr::null_mut(),
    avail: ptr::null_mut(),
    used: ptr::null_mut(),
    last_used: 0,
    rx_bufs: [ptr::null_mut(); RX_DESCS],
    mac: [0; 6],
};

pub unsafe fn probe(base: usize) -> bool {
    if reg_r(base, R_MAGIC_VALUE) != 0x7472_6976 {
        return false;
    }
    reg_r(base, R_DEVICE_ID) == VIRTIO_ID_NET
}

pub unsafe fn init(base: usize) -> KResult<()> {
    if G_NET.base != 0 {
        return Err(Errno::Busy);
    }
    let version = reg_r(base, R_VERSION);
    let modern = version >= 2;
    G_NET.base = base;
    G_NET.modern = modern;
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
    // MAC address lives at device-specific config offset 0x100 in legacy MMIO.
    for i in 0..6 {
        G_NET.mac[i] = Mmio::<u8>::at(base + 0x100 + i).read();
    }
    setup_rx_queue()?;
    reg_w(
        base,
        R_STATUS,
        VIRTIO_S_ACK | VIRTIO_S_DRIVER | VIRTIO_S_DRIVER_OK,
    );
    Ok(())
}

unsafe fn setup_rx_queue() -> KResult<()> {
    let base = G_NET.base;
    reg_w(base, R_QUEUE_SEL, 0);
    reg_w(base, R_QUEUE_NUM, VIRTQ_SIZE as u32);
    let desc_pa = pmm::alloc_zero()? as usize;
    let avail_pa = pmm::alloc_zero()? as usize;
    let used_pa = pmm::alloc_zero()? as usize;
    G_NET.desc = desc_pa as *mut VqDesc;
    G_NET.avail = avail_pa as *mut VqAvail;
    G_NET.used = used_pa as *mut VqUsed;
    reg_w(base, R_QUEUE_DESC_LOW, desc_pa as u32);
    reg_w(base, R_QUEUE_DESC_HIGH, ((desc_pa as u64) >> 32) as u32);
    reg_w(base, R_QUEUE_AVAIL_LOW, avail_pa as u32);
    reg_w(base, R_QUEUE_AVAIL_HIGH, ((avail_pa as u64) >> 32) as u32);
    reg_w(base, R_QUEUE_USED_LOW, used_pa as u32);
    reg_w(base, R_QUEUE_USED_HIGH, ((used_pa as u64) >> 32) as u32);
    reg_w(base, R_QUEUE_ENABLE, 1);
    for i in 0..RX_DESCS {
        let buf_pa = pmm::alloc_zero()? as *mut u8;
        G_NET.rx_bufs[i] = buf_pa;
        (*G_NET.desc.add(i)) = VqDesc {
            addr: buf_pa as u64,
            len: (HDR_LEN + NET_MTU) as u32,
            flags: VQ_DESC_F_WRITE,
            next: 0,
        };
        push_avail(i);
    }
    Ok(())
}

pub(crate) unsafe fn push_avail(idx: usize) {
    let i = ptr::read_volatile(ptr::addr_of!((*G_NET.avail).idx));
    ptr::write_volatile(
        ptr::addr_of_mut!((*G_NET.avail).ring[(i as usize) % VIRTQ_SIZE]),
        idx as u16,
    );
    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
    ptr::write_volatile(ptr::addr_of_mut!((*G_NET.avail).idx), i.wrapping_add(1));
}

pub fn mac() -> [u8; 6] {
    unsafe { G_NET.mac }
}

pub mod xfer;
pub use xfer::{recv_into, send};

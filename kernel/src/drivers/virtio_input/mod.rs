//! virtio-input MMIO driver — root module.
//!
//! Probe/init + event decode. Reads raw input events (keyboard, mouse,
//! tablet) from the device's event virtqueue and translates them to
//! the unified `drivers::input::Event` representation.
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

pub const VIRTIO_ID_INPUT: u32 = 18;
pub const N_EVENTS: usize = 8;

/// virtio-input event (spec §5.5.5).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct VirtioInputEvent {
    pub type_: u16,
    pub code: u16,
    pub value: u32,
}

#[derive(Clone, Copy)]
pub(crate) struct InDev {
    pub base: usize,
    pub modern: bool,
    pub desc: *mut VqDesc,
    pub avail: *mut VqAvail,
    pub used: *mut VqUsed,
    pub last_used: u16,
    pub ev_buf: *mut VirtioInputEvent,
    pub _head: usize,
}

pub(crate) static mut G_IN: InDev = InDev {
    base: 0,
    modern: false,
    desc: ptr::null_mut(),
    avail: ptr::null_mut(),
    used: ptr::null_mut(),
    last_used: 0,
    ev_buf: ptr::null_mut(),
    _head: 0,
};

pub unsafe fn probe(base: usize) -> bool {
    if reg_r(base, R_MAGIC_VALUE) != 0x7472_6976 {
        return false;
    }
    reg_r(base, R_DEVICE_ID) == VIRTIO_ID_INPUT
}

pub unsafe fn init(base: usize) -> KResult<()> {
    if G_IN.base != 0 {
        return Err(Errno::Busy);
    }
    let version = reg_r(base, R_VERSION);
    let modern = version >= 2;
    G_IN.base = base;
    G_IN.modern = modern;
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
    setup_event_queue()?;
    reg_w(base, R_STATUS, VIRTIO_S_ACK | VIRTIO_S_DRIVER | VIRTIO_S_DRIVER_OK);
    Ok(())
}

unsafe fn setup_event_queue() -> KResult<()> {
    let base = G_IN.base;
    reg_w(base, R_QUEUE_SEL, 0);
    reg_w(base, R_QUEUE_NUM, VIRTQ_SIZE as u32);
    let desc_pa = pmm::alloc_zero()? as usize;
    let avail_pa = pmm::alloc_zero()? as usize;
    let used_pa = pmm::alloc_zero()? as usize;
    let ev_pa = pmm::alloc_zero()? as *mut VirtioInputEvent;
    G_IN.desc = desc_pa as *mut VqDesc;
    G_IN.avail = avail_pa as *mut VqAvail;
    G_IN.used = used_pa as *mut VqUsed;
    G_IN.ev_buf = ev_pa;
    reg_w(base, R_QUEUE_DESC_LOW, desc_pa as u32);
    reg_w(base, R_QUEUE_DESC_HIGH, ((desc_pa as u64) >> 32) as u32);
    reg_w(base, R_QUEUE_AVAIL_LOW, avail_pa as u32);
    reg_w(base, R_QUEUE_AVAIL_HIGH, ((avail_pa as u64) >> 32) as u32);
    reg_w(base, R_QUEUE_USED_LOW, used_pa as u32);
    reg_w(base, R_QUEUE_USED_HIGH, ((used_pa as u64) >> 32) as u32);
    reg_w(base, R_QUEUE_ENABLE, 1);
    // Pre-post N_EVENTS descriptors for incoming events.
    for i in 0..N_EVENTS {
        let off = (ev_pa as usize) + i * core::mem::size_of::<VirtioInputEvent>();
        (*G_IN.desc.add(i)) = VqDesc {
            addr: off as u64,
            len: core::mem::size_of::<VirtioInputEvent>() as u32,
            flags: VQ_DESC_F_WRITE,
            next: 0,
        };
        push(i);
    }
    Ok(())
}

unsafe fn push(idx: usize) {
    let i = ptr::read_volatile(ptr::addr_of!((*G_IN.avail).idx));
    ptr::write_volatile(
        ptr::addr_of_mut!((*G_IN.avail).ring[(i as usize) % VIRTQ_SIZE]),
        idx as u16,
    );
    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
    ptr::write_volatile(ptr::addr_of_mut!((*G_IN.avail).idx), i.wrapping_add(1));
}

pub mod decode;
pub use decode::{poll, poll_unified, EventType};

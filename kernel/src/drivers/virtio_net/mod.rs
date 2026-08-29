//! virtio-net MMIO driver — root module.
//!
//! Owns the device struct, global state, register constants, and the
//! probe / init sequence. Frame I/O lives in `xfer.rs`.
use crate::arch::mmio::Mmio;
use crate::drivers::virtio::{
    R_DEVICE_ID, R_GUEST_FEATURES, R_GUEST_FEATURES_SEL, R_HOST_FEATURES, R_HOST_FEATURES_SEL,
    R_MAGIC_VALUE, R_QUEUE_AVAIL_HIGH, R_QUEUE_AVAIL_LOW, R_QUEUE_DESC_HIGH, R_QUEUE_DESC_LOW,
    R_QUEUE_NUM, R_QUEUE_READY, R_QUEUE_SEL, R_QUEUE_USED_HIGH, R_QUEUE_USED_LOW, R_STATUS,
    R_VERSION, VIRTIO_F_VERSION_1, VIRTIO_S_ACK, VIRTIO_S_DRIVER, VIRTIO_S_DRIVER_OK,
    VIRTIO_S_FEATURES_OK, VIRTQ_SIZE, VQ_DESC_F_WRITE, VqAvail, VqDesc, VqUsed, reg_r, reg_w,
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

/// True once a virtio-net device has been fully initialized (queues up).
pub fn present() -> bool {
    // SAFETY: reads of base/desc written together by single-threaded boot init; kernel code never runs with SIE set (see crate::sync).
    unsafe { G_NET.base != 0 && !G_NET.desc.is_null() }
}

/// # Safety
///
/// `base` must be a candidate virtio-mmio base from the FDT probe or the
/// QEMU virt fallback constants (identity-mapped at boot).
pub unsafe fn probe(base: usize) -> bool {
    // SAFETY: base is a candidate virtio-mmio base from the boot-time probe; reg_r reads only spec offsets (magic, device ID).
    unsafe {
        if reg_r(base, R_MAGIC_VALUE) != 0x7472_6976 {
            return false;
        }
        reg_r(base, R_DEVICE_ID) == VIRTIO_ID_NET
    }
}

/// # Safety
///
/// `base` must be a probed virtio-net MMIO base; must be called during the
/// single-threaded boot-time device probe, once per base.
pub unsafe fn init(base: usize) -> KResult<()> {
    // SAFETY: boot-time single-threaded probe (SIE=0, see crate::sync) on a probed base, Busy-guarded so G_NET is written at most once; offsets are spec constants and MAC reads target the device config region at 0x100 (legacy MMIO layout).
    unsafe {
        if G_NET.base != 0 {
            return Err(Errno::Busy);
        }
        let version = reg_r(base, R_VERSION);
        let modern = version >= 2;
        reg_w(base, R_STATUS, 0);
        reg_w(base, R_STATUS, VIRTIO_S_ACK | VIRTIO_S_DRIVER);
        // Read the full 64-bit feature word via the features_sel registers and
        // negotiate VIRTIO_F_VERSION_1 (bit 32) explicitly: sedna's virtio
        // emulation clears FEATURES_OK unless that bit is present in the guest
        // features, which would fail the device (and leave G_NET half-initialized
        // in older code, faulting on a NULL descriptor queue later).
        reg_w(base, R_HOST_FEATURES_SEL, 1);
        let host_feat_hi = reg_r(base, R_HOST_FEATURES);
        reg_w(base, R_HOST_FEATURES_SEL, 0);
        let host_feat_lo = reg_r(base, R_HOST_FEATURES);
        let guest_hi = host_feat_hi | VIRTIO_F_VERSION_1;
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
                crate::kwrn!("virtio-net", "device did not set FEATURES_OK, continuing");
            }
        }
        // MAC address lives at device-specific config offset 0x100 in legacy MMIO.
        for i in 0..6 {
            G_NET.mac[i] = Mmio::<u8>::at(base + 0x100 + i).read();
        }
        // Only expose the device to the network stack once the queues are fully
        // set up — G_NET.base non-zero is the "ready" signal used by present().
        setup_rx_queue(base)?;
        G_NET.base = base;
        G_NET.modern = modern;
        reg_w(
            base,
            R_STATUS,
            VIRTIO_S_ACK | VIRTIO_S_DRIVER | VIRTIO_S_DRIVER_OK,
        );
        Ok(())
    }
}

/// # Safety
///
/// `base` must be the probed virtio-net base already reset/ack'd by `init`;
/// must be called during single-threaded boot-time probe before G_NET.base
/// is published.
unsafe fn setup_rx_queue(base: usize) -> KResult<()> {
    // SAFETY: called from init on a probed base; rings and RX_DESCS RX buffers are fresh contiguous PMM pages registered with the device (LOW+HIGH / QUEUE_READY) before use; desc slots 0..RX_DESCS < VIRTQ_SIZE.
    unsafe {
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
        reg_w(base, R_QUEUE_READY, 1);
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
}

/// # Safety
///
/// The avail ring must have been set up by `init` (null-checked inside);
/// `idx` must be a valid descriptor index for that queue (< RX_DESCS for RX).
pub(crate) unsafe fn push_avail(idx: usize) {
    // SAFETY: G_NET.avail is a PMM ring set up by setup_rx_queue; ring slot masked % VIRTQ_SIZE per spec; volatile write + SeqCst fence order entry before idx bump.
    unsafe {
        if G_NET.avail.is_null() {
            return;
        }
        let i = ptr::read_volatile(ptr::addr_of!((*G_NET.avail).idx));
        ptr::write_volatile(
            ptr::addr_of_mut!((*G_NET.avail).ring[(i as usize) % VIRTQ_SIZE]),
            idx as u16,
        );
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        ptr::write_volatile(ptr::addr_of_mut!((*G_NET.avail).idx), i.wrapping_add(1));
    }
}

pub fn mac() -> [u8; 6] {
    // SAFETY: copy of the 6-byte MAC written during single-threaded boot init; kernel code never runs with SIE set (see crate::sync).
    unsafe { G_NET.mac }
}

pub mod xfer;
pub use xfer::{recv_into, send};

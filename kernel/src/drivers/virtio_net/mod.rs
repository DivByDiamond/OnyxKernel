//! virtio-net MMIO driver — root module.
//!
//! Owns the device struct, global state, register constants, and the
//! probe / init sequence. Frame I/O lives in `xfer.rs`.
use crate::arch::mmio::Mmio;
use crate::drivers::virtio::{
    R_DEVICE_ID, R_GUEST_FEATURES, R_GUEST_FEATURES_SEL, R_GUEST_PAGE_SIZE, R_HOST_FEATURES,
    R_HOST_FEATURES_SEL, R_MAGIC_VALUE, R_QUEUE_ALIGN, R_QUEUE_AVAIL_HIGH, R_QUEUE_AVAIL_LOW,
    R_QUEUE_DESC_HIGH, R_QUEUE_DESC_LOW, R_QUEUE_NUM, R_QUEUE_PFN, R_QUEUE_READY, R_QUEUE_SEL,
    R_QUEUE_USED_HIGH, R_QUEUE_USED_LOW, R_STATUS, R_VERSION, VIRTIO_F_VERSION_1, VIRTIO_S_ACK,
    VIRTIO_S_DRIVER, VIRTIO_S_DRIVER_OK, VIRTIO_S_FEATURES_OK, VIRTQ_SIZE, VQ_DESC_F_WRITE,
    VqAvail, VqDesc, VqUsed, reg_r, reg_w,
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
    // Queue 0 — receiveq (device-writable buffers we pre-post).
    pub rx_desc: *mut VqDesc,
    pub rx_avail: *mut VqAvail,
    pub rx_used: *mut VqUsed,
    pub rx_last_used: u16,
    pub rx_bufs: [*mut u8; RX_DESCS],
    // Queue 1 — transmitq (a *separate* ring; sending through the RX
    // queue's rings — the original bug here — clobbers RX buffers and
    // notifies the device of "new RX space" instead of "frame to send",
    // so nothing ever reaches the wire; see OnyxKernel/todo.md).
    pub tx_desc: *mut VqDesc,
    pub tx_avail: *mut VqAvail,
    pub tx_used: *mut VqUsed,
    pub _tx_last_used: u16,
    pub mac: [u8; 6],
}

pub(crate) static mut G_NET: NetDev = NetDev {
    base: 0,
    modern: false,
    rx_desc: ptr::null_mut(),
    rx_avail: ptr::null_mut(),
    rx_used: ptr::null_mut(),
    rx_last_used: 0,
    rx_bufs: [ptr::null_mut(); RX_DESCS],
    tx_desc: ptr::null_mut(),
    tx_avail: ptr::null_mut(),
    tx_used: ptr::null_mut(),
    _tx_last_used: 0,
    mac: [0; 6],
};

/// True once a virtio-net device has been fully initialized (queues up).
pub fn present() -> bool {
    // SAFETY: reads of base/rx_desc written together by single-threaded boot init; kernel code never runs with SIE set (see crate::sync).
    unsafe { G_NET.base != 0 && !G_NET.rx_desc.is_null() }
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
        crate::kinf!(
            "virtio-net",
            "mmio version=%d modern=%d",
            crate::srv::klog::FmtArg::from(version),
            crate::srv::klog::FmtArg::from(modern as u32)
        );
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
        setup_rx_queue(base, modern)?;
        setup_tx_queue(base, modern)?;
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
unsafe fn setup_rx_queue(base: usize, modern: bool) -> KResult<()> {
    // SAFETY: called from init on a probed base; rings and RX_DESCS RX buffers are fresh contiguous PMM pages registered with the device before use (LOW+HIGH/QUEUE_READY on modern MMIO, a single contiguous QUEUE_PFN region on legacy); desc slots 0..RX_DESCS < VIRTQ_SIZE.
    unsafe {
        reg_w(base, R_QUEUE_SEL, 0);
        reg_w(base, R_QUEUE_NUM, VIRTQ_SIZE as u32);
        let (desc_pa, avail_pa, used_pa) = setup_queue_rings(base, modern)?;
        G_NET.rx_desc = desc_pa as *mut VqDesc;
        G_NET.rx_avail = avail_pa as *mut VqAvail;
        G_NET.rx_used = used_pa as *mut VqUsed;
        for i in 0..RX_DESCS {
            let buf_pa = pmm::alloc_zero()? as *mut u8;
            G_NET.rx_bufs[i] = buf_pa;
            (*G_NET.rx_desc.add(i)) = VqDesc {
                addr: buf_pa as u64,
                len: (HDR_LEN + NET_MTU) as u32,
                flags: VQ_DESC_F_WRITE,
                next: 0,
            };
            push_avail(i, true);
        }
        Ok(())
    }
}

/// # Safety
///
/// `base` must be the probed virtio-net base already reset/ack'd by `init`,
/// with `setup_rx_queue` already having claimed queue 0; must be called
/// during single-threaded boot-time probe before G_NET.base is published.
///
/// virtio-net (like virtio-console) has separate RX (queue 0) and TX
/// (queue 1) virtqueues — each is a distinct desc/avail/used ring the
/// driver must select via R_QUEUE_SEL and register independently. Reusing
/// the RX rings for outbound frames (the original bug: `send()` used to
/// write into `G_NET.desc`/`avail`/`used`, i.e. the RX queue set up here as
/// queue 0, and notify queue 0) clobbers RX buffers and tells the device
/// "more RX space is available", not "here is a frame to transmit" — so no
/// Ethernet frame ever reaches the wire, which is exactly what a pcap
/// capture on the netdev showed (zero packets, even for DHCP).
unsafe fn setup_tx_queue(base: usize, modern: bool) -> KResult<()> {
    // SAFETY: called from init on a probed base after setup_rx_queue; rings are a fresh PMM region registered with the device before use (LOW+HIGH/QUEUE_READY on modern MMIO, a single contiguous QUEUE_PFN region on legacy); queue index 1 is virtio-net's spec-fixed transmitq.
    unsafe {
        reg_w(base, R_QUEUE_SEL, 1);
        reg_w(base, R_QUEUE_NUM, VIRTQ_SIZE as u32);
        let (desc_pa, avail_pa, used_pa) = setup_queue_rings(base, modern)?;
        G_NET.tx_desc = desc_pa as *mut VqDesc;
        G_NET.tx_avail = avail_pa as *mut VqAvail;
        G_NET.tx_used = used_pa as *mut VqUsed;
        Ok(())
    }
}

/// Allocates and registers one virtqueue's desc/avail/used rings with the
/// device already `R_QUEUE_SEL`/`R_QUEUE_NUM`-selected by the caller.
/// Returns their physical addresses.
///
/// # Safety
///
/// Caller must have already written `R_QUEUE_SEL` and `R_QUEUE_NUM` for the
/// queue being set up; must run during single-threaded boot-time probe.
///
/// Two incompatible virtio-mmio register layouts exist, selected by
/// `R_VERSION`: **modern** (v2+, `modern == true`) writes each ring's
/// address independently via `R_QUEUE_{DESC,AVAIL,USED}_{LOW,HIGH}` and
/// flips `R_QUEUE_READY`. **Legacy** (v1, `modern == false`) has none of
/// those registers — real `qemu-system-riscv64 -device virtio-net-device`
/// defaults to this transport — and instead expects desc/avail/used as one
/// physically **contiguous**, page-aligned region, communicated with a
/// single `R_QUEUE_PFN` write (the page frame number of the `desc` ring;
/// avail/used offsets within the region are implied by the spec's legacy
/// layout) after declaring `R_GUEST_PAGE_SIZE` and `R_QUEUE_ALIGN`. The
/// original driver only ever did the modern half, so on legacy transport
/// (as real QEMU presents by default) `R_QUEUE_READY` is a no-op write to
/// an unmapped-in-spec offset and the queue is never actually armed — no
/// frame the driver "sends" ever reaches the wire, which is exactly what a
/// pcap capture on the netdev showed (zero packets, even for DHCP). See
/// `kernel/src/drivers/virtio/queue.rs::setup_queue` for the virtio-blk
/// driver already handling both paths — this mirrors that.
unsafe fn setup_queue_rings(base: usize, modern: bool) -> KResult<(usize, usize, usize)> {
    // SAFETY: modern path: three independent PMM pages, addresses handed to the device before any descriptor is filled in by the caller. Legacy path: one 3-page contiguous PMM region laid out desc|avail|used (each ring fits in a page at VIRTQ_SIZE), PFN computed against the 4096 byte guest page size declared just above it.
    unsafe {
        if modern {
            let desc_pa = pmm::alloc_zero()? as usize;
            let avail_pa = pmm::alloc_zero()? as usize;
            let used_pa = pmm::alloc_zero()? as usize;
            reg_w(base, R_QUEUE_DESC_LOW, desc_pa as u32);
            reg_w(base, R_QUEUE_DESC_HIGH, ((desc_pa as u64) >> 32) as u32);
            reg_w(base, R_QUEUE_AVAIL_LOW, avail_pa as u32);
            reg_w(base, R_QUEUE_AVAIL_HIGH, ((avail_pa as u64) >> 32) as u32);
            reg_w(base, R_QUEUE_USED_LOW, used_pa as u32);
            reg_w(base, R_QUEUE_USED_HIGH, ((used_pa as u64) >> 32) as u32);
            reg_w(base, R_QUEUE_READY, 1);
            Ok((desc_pa, avail_pa, used_pa))
        } else {
            let contig_pa = pmm::alloc_n(3)? as usize;
            let desc_pa = contig_pa;
            let avail_pa = contig_pa + 4096;
            let used_pa = contig_pa + 2 * 4096;
            reg_w(base, R_GUEST_PAGE_SIZE, 4096);
            reg_w(base, R_QUEUE_ALIGN, 4096);
            reg_w(base, R_QUEUE_PFN, (desc_pa / 4096) as u32);
            Ok((desc_pa, avail_pa, used_pa))
        }
    }
}

/// # Safety
///
/// The target ring must have been set up by `init` (null-checked inside);
/// `idx` must be a valid descriptor index for that queue (< RX_DESCS for
/// RX; TX only ever posts slot 0, one in-flight frame at a time).
pub(crate) unsafe fn push_avail(idx: usize, is_rx: bool) {
    // SAFETY: G_NET.{rx,tx}_avail are PMM rings set up by setup_{rx,tx}_queue; ring slot masked % VIRTQ_SIZE per spec; volatile write + SeqCst fence order entry before idx bump.
    unsafe {
        let avail = if is_rx {
            G_NET.rx_avail
        } else {
            G_NET.tx_avail
        };
        if avail.is_null() {
            return;
        }
        let i = ptr::read_volatile(ptr::addr_of!((*avail).idx));
        ptr::write_volatile(
            ptr::addr_of_mut!((*avail).ring[(i as usize) % VIRTQ_SIZE]),
            idx as u16,
        );
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        ptr::write_volatile(ptr::addr_of_mut!((*avail).idx), i.wrapping_add(1));
    }
}

pub fn mac() -> [u8; 6] {
    // SAFETY: copy of the 6-byte MAC written during single-threaded boot init; kernel code never runs with SIE set (see crate::sync).
    unsafe { G_NET.mac }
}

pub mod xfer;
pub use xfer::{recv_into, send};

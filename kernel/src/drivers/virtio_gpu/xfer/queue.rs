//! Control-queue setup and the low-level synchronous descriptor kick.
use super::GPU_QSIZE;
use super::R_OK;
use crate::drivers::virtio::{
    R_GUEST_PAGE_SIZE, R_QUEUE_ALIGN, R_QUEUE_AVAIL_HIGH, R_QUEUE_AVAIL_LOW, R_QUEUE_DESC_HIGH,
    R_QUEUE_DESC_LOW, R_QUEUE_NOTIFY, R_QUEUE_NUM, R_QUEUE_NUM_MAX, R_QUEUE_PFN, R_QUEUE_READY,
    R_QUEUE_SEL, R_QUEUE_USED_HIGH, R_QUEUE_USED_LOW, VQ_DESC_F_WRITE, VqAvail, VqDesc, VqUsed,
    reg_r, reg_w,
};
use crate::mm::pmm;
use onyx_core::errno::{Errno, KResult};

/// # Safety
///
/// `d`/`a`/`u` must be valid writable locations for the queue pointers; `b` a probed, ack'd virtio-mmio base.
pub unsafe fn setup_queue(
    d: *mut *mut VqDesc,
    a: *mut *mut VqAvail,
    u: *mut *mut VqUsed,
    b: usize,
) -> KResult<()> {
    // SAFETY: d/a/u valid per contract; rings are fresh contiguous PMM pages stored through them and registered with the device before use; offsets are spec constants.
    unsafe {
        let ver = reg_r(b, crate::drivers::virtio::R_VERSION);
        let modern = ver >= 2;
        crate::kinf!(
            "virtio-gpu",
            "queue setup ver=%d modern=%d base=%p",
            onyx_core::fmt::Arg::from(ver),
            onyx_core::fmt::Arg::from(modern as u32),
            onyx_core::fmt::Arg::from(b)
        );
        // Queue 0 — control queue (the only one we use). For legacy we only
        // configure q0; q1 (cursor) is optional and not needed for scanout.
        let qsel = 0u32;
        reg_w(b, R_QUEUE_SEL, qsel);
        let max = reg_r(b, R_QUEUE_NUM_MAX);
        let qsize = if max == 0 {
            GPU_QSIZE as u32
        } else {
            core::cmp::min(GPU_QSIZE as u32, max)
        };
        crate::kinf!(
            "virtio-gpu",
            "q%d max=%d qsize=%d",
            onyx_core::fmt::Arg::from(qsel),
            onyx_core::fmt::Arg::from(max),
            onyx_core::fmt::Arg::from(qsize)
        );
        if qsize != 0 {
            reg_w(b, R_QUEUE_NUM, qsize);
            if modern {
                let dp = pmm::alloc_zero()? as usize;
                let ap = pmm::alloc_zero()? as usize;
                let up = pmm::alloc_zero()? as usize;
                *d = dp as *mut VqDesc;
                *a = ap as *mut VqAvail;
                *u = up as *mut VqUsed;
                reg_w(b, R_QUEUE_DESC_LOW, dp as u32);
                reg_w(b, R_QUEUE_DESC_HIGH, ((dp as u64) >> 32) as u32);
                reg_w(b, R_QUEUE_AVAIL_LOW, ap as u32);
                reg_w(b, R_QUEUE_AVAIL_HIGH, ((ap as u64) >> 32) as u32);
                reg_w(b, R_QUEUE_USED_LOW, up as u32);
                reg_w(b, R_QUEUE_USED_HIGH, ((up as u64) >> 32) as u32);
                reg_w(b, R_QUEUE_READY, 1);
            } else {
                let contig = pmm::alloc_n(3)? as usize;
                let dp = contig;
                *d = dp as *mut VqDesc;
                *a = (contig + 4096) as *mut VqAvail;
                *u = (contig + 8192) as *mut VqUsed;
                reg_w(b, R_GUEST_PAGE_SIZE, 4096);
                reg_w(b, R_QUEUE_ALIGN, 4096);
                reg_w(b, R_QUEUE_PFN, (dp / 4096) as u32);
                crate::kinf!(
                    "virtio-gpu",
                    "legacy PFN=%x",
                    onyx_core::fmt::Arg::from((dp / 4096) as u32)
                );
            }
        }
        // Ensure queue 0 is selected for subsequent kicks.
        reg_w(b, R_QUEUE_SEL, 0);
        crate::kinf!(
            "virtio-gpu",
            "queue ready status=%x",
            onyx_core::fmt::Arg::from(crate::drivers::virtio::reg_r(
                b,
                crate::drivers::virtio::R_STATUS
            ))
        );
        Ok(())
    }
}

/// # Safety
///
/// `d`/`a`/`u` must point at a queue initialized by `setup_queue`, `lu` at the last-used tracker; `rp` a PMM page for the 24-byte response.
pub(super) unsafe fn kick(
    d: *mut VqDesc,
    a: *mut VqAvail,
    u: *mut VqUsed,
    lu: *mut u16,
    b: usize,
    rp: usize,
) -> KResult<()> {
    // SAFETY: queue pointers per contract; desc/avail slots masked % GPU_QSIZE; rp is a PMM page so the 24-byte repr(C) response read is in bounds; SeqCst fences order avail updates and completion reads.
    unsafe {
        use core::ptr::{read_volatile, write_volatile};
        use core::sync::atomic::{Ordering, fence};
        let avail_idx_ptr = core::ptr::addr_of!((*a).idx);
        let used_idx_ptr = core::ptr::addr_of!((*u).idx);
        let i = read_volatile(avail_idx_ptr) as usize % GPU_QSIZE;
        *d.add((i + 1) % GPU_QSIZE) = VqDesc {
            addr: rp as u64,
            len: 24,
            flags: VQ_DESC_F_WRITE,
            next: 0,
        };
        let idx = read_volatile(avail_idx_ptr);
        write_volatile(
            core::ptr::addr_of_mut!((*a).ring[(idx as usize) % GPU_QSIZE]),
            i as u16,
        );
        fence(Ordering::SeqCst);
        write_volatile(avail_idx_ptr as *mut u16, idx.wrapping_add(1));
        reg_w(b, R_QUEUE_NOTIFY, 0);
        let mut spins = 0u64;
        loop {
            fence(Ordering::SeqCst);
            let used = read_volatile(used_idx_ptr);
            if used != *lu {
                *lu = used;
                let resp = read_volatile(rp as *const u32);
                if resp == R_OK {
                    return Ok(());
                }
                crate::kwrn!(
                    "virtio-gpu",
                    "kick: device resp=0x%x expected 0x%x",
                    onyx_core::fmt::Arg::from(resp),
                    onyx_core::fmt::Arg::from(R_OK)
                );
                return Err(Errno::Io);
            }
            spins += 1;
            if spins == 500_000 {
                crate::kwrn!(
                    "virtio-gpu",
                    "kick: spins=500k avail=0x%x used=0x%x",
                    onyx_core::fmt::Arg::from(read_volatile(avail_idx_ptr) as u32),
                    onyx_core::fmt::Arg::from(read_volatile(used_idx_ptr) as u32)
                );
                // Keep spinning — like virtio-blk, the device should eventually complete.
                // To avoid infinite hang during debugging, break after 5M spins.
                if spins > 5_000_000 {
                    crate::kwrn!("virtio-gpu", "kick: timeout after 5M spins");
                    return Err(Errno::Io);
                }
            }
            if spins > 5_000_000 {
                return Err(Errno::Io);
            }
        }
    }
}

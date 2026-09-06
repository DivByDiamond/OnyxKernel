use super::{C_RESOURCE_CREATE_2D, C_SET_SCANOUT, Create2D, GpuCtrlHdr, SetScanout};
use crate::drivers::virtio::{
    R_GUEST_PAGE_SIZE, R_QUEUE_ALIGN, R_QUEUE_AVAIL_HIGH, R_QUEUE_AVAIL_LOW, R_QUEUE_DESC_HIGH,
    R_QUEUE_DESC_LOW, R_QUEUE_NOTIFY, R_QUEUE_NUM, R_QUEUE_NUM_MAX, R_QUEUE_PFN, R_QUEUE_READY,
    R_QUEUE_SEL, R_QUEUE_USED_HIGH, R_QUEUE_USED_LOW, VQ_DESC_F_NEXT, VQ_DESC_F_WRITE, VqAvail,
    VqDesc, VqUsed, reg_r, reg_w,
};
use crate::mm::pmm;
use onyx_core::errno::{Errno, KResult};

const R_OK: u32 = 0x1100;
const C_ATTACH: u32 = 0x106;
const GPU_QSIZE: usize = 64;
const C_GET_DISPLAY_INFO: u32 = 0x100;

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
unsafe fn kick(
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

/// # Safety
///
/// `cmd` must point to `len` bytes of device-accessible (physical/PMM) memory valid for the synchronous kick; queue pointers per `kick`.
pub unsafe fn send_cmd(
    d: *mut VqDesc,
    a: *mut VqAvail,
    u: *mut VqUsed,
    lu: *mut u16,
    b: usize,
    cmd: *mut u8,
    len: u32,
) -> KResult<()> {
    // SAFETY: queue pointers per kick contract; slots masked % VIRTQ_SIZE; cmd is device-accessible for len bytes per the caller contract.
    unsafe {
        let rp = pmm::alloc_zero()? as usize;
        let i = (*a).idx as usize % GPU_QSIZE;
        *d.add(i) = VqDesc {
            addr: cmd as u64,
            len,
            flags: VQ_DESC_F_NEXT,
            next: ((i + 1) % GPU_QSIZE) as u16,
        };
        kick(d, a, u, lu, b, rp)
    }
}

/// # Safety
///
/// Queue pointers per `kick`; the command struct must outlive the synchronous kick (true for the stack local).
pub unsafe fn cmd_create2d(
    d: *mut VqDesc,
    a: *mut VqAvail,
    u: *mut VqUsed,
    lu: *mut u16,
    b: usize,
    rid: u32,
    w: u32,
    h: u32,
) -> KResult<()> {
    // SAFETY: pmm buffer holds the command for the synchronous kick; layout matches spec.
    unsafe {
        let buf = pmm::alloc_zero()? as usize;
        let c = Create2D {
            hdr: GpuCtrlHdr {
                hdr_type: C_RESOURCE_CREATE_2D,
                flags: 0,
                fence_id: 0,
                ctx_id: 0,
                padding: 0,
            },
            resource_id: rid,
            format: 1, // VIRTIO_GPU_FORMAT_B8G8R8A8_UNORM
            width: w,
            height: h,
        };
        core::ptr::copy_nonoverlapping(
            &c as *const _ as *const u8,
            buf as *mut u8,
            core::mem::size_of::<Create2D>(),
        );
        let ret = send_cmd(
            d,
            a,
            u,
            lu,
            b,
            buf as *mut u8,
            core::mem::size_of::<Create2D>() as u32,
        );
        // Leak the buffer for the duration of the synchronous call; free not needed as kick returns.
        // We intentionally leak the single page (zeroed) — pmm will reclaim on reboot.
        ret
    }
}

/// # Safety
///
/// `rid`/`pa`/`len` must describe a device resource backed by `len` bytes at physical `pa`; queue pointers per `kick`.
pub unsafe fn cmd_attach(
    d: *mut VqDesc,
    a: *mut VqAvail,
    u: *mut VqUsed,
    lu: *mut u16,
    b: usize,
    rid: u32,
    pa: u32,
    len: u32,
) -> KResult<()> {
    // SAFETY: bp is a fresh PMM page so the header copy and writes at offsets 24..44 are in bounds; pa/len describe the PMM framebuffer per contract; desc slots masked % VIRTQ_SIZE.
    unsafe {
        let buf = pmm::alloc_zero()? as usize;
        let bp = buf as *mut u8;
        let h = GpuCtrlHdr {
            hdr_type: C_ATTACH,
            flags: 0,
            fence_id: 0,
            ctx_id: 0,
            padding: 0,
        };
        core::ptr::copy_nonoverlapping(&h as *const _ as *const u8, bp, 24);
        *(bp.add(24) as *mut u32) = rid;
        *(bp.add(28) as *mut u32) = 1;
        *(bp.add(32) as *mut u64) = pa as u64;
        *(bp.add(40) as *mut u32) = len;
        *(bp.add(44) as *mut u32) = 0; // padding per spec
        let rp = pmm::alloc_zero()? as usize;
        let i = (*a).idx as usize % GPU_QSIZE;
        *d.add(i) = VqDesc {
            addr: buf as u64,
            len: 48,
            flags: VQ_DESC_F_NEXT,
            next: ((i + 1) % GPU_QSIZE) as u16,
        };
        *d.add((i + 1) % GPU_QSIZE) = VqDesc {
            addr: rp as u64,
            len: 24,
            flags: VQ_DESC_F_WRITE,
            next: 0,
        };
        kick(d, a, u, lu, b, rp)
    }
}

/// # Safety
///
/// Queue pointers per `kick`; the command struct must outlive the synchronous kick (true for the stack local).
pub unsafe fn cmd_scanout(
    d: *mut VqDesc,
    a: *mut VqAvail,
    u: *mut VqUsed,
    lu: *mut u16,
    b: usize,
    rid: u32,
    w: u32,
    h: u32,
) -> KResult<()> {
    // SAFETY: pmm buffer holds the command for the synchronous kick; layout matches spec.
    unsafe {
        let buf = pmm::alloc_zero()? as usize;
        let c = SetScanout {
            hdr: GpuCtrlHdr {
                hdr_type: C_SET_SCANOUT,
                flags: 0,
                fence_id: 0,
                ctx_id: 0,
                padding: 0,
            },
            rect_x: 0,
            rect_y: 0,
            rect_w: w,
            rect_h: h,
            scanout_id: 0,
            resource_id: rid,
        };
        core::ptr::copy_nonoverlapping(
            &c as *const _ as *const u8,
            buf as *mut u8,
            core::mem::size_of::<SetScanout>(),
        );
        send_cmd(
            d,
            a,
            u,
            lu,
            b,
            buf as *mut u8,
            core::mem::size_of::<SetScanout>() as u32,
        )
    }
}

pub unsafe fn cmd_get_display_info(
    d: *mut VqDesc,
    a: *mut VqAvail,
    u: *mut VqUsed,
    lu: *mut u16,
    b: usize,
) -> KResult<()> {
    // SAFETY: pmm buffer holds the command; response is 408 bytes (header + 16 pmodes).
    unsafe {
        let cmd_buf = pmm::alloc_zero()? as usize;
        let resp_buf = pmm::alloc_zero()? as usize;
        let h = GpuCtrlHdr {
            hdr_type: C_GET_DISPLAY_INFO,
            flags: 0,
            fence_id: 0,
            ctx_id: 0,
            padding: 0,
        };
        core::ptr::copy_nonoverlapping(&h as *const _ as *const u8, cmd_buf as *mut u8, 24);
        use core::ptr::{read_volatile, write_volatile};
        use core::sync::atomic::{Ordering, fence};
        let avail_idx_ptr = core::ptr::addr_of!((*a).idx);
        let i = read_volatile(avail_idx_ptr) as usize % GPU_QSIZE;
        *d.add(i) = VqDesc {
            addr: cmd_buf as u64,
            len: 24,
            flags: VQ_DESC_F_NEXT,
            next: ((i + 1) % GPU_QSIZE) as u16,
        };
        *d.add((i + 1) % GPU_QSIZE) = VqDesc {
            addr: resp_buf as u64,
            len: 408,
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
        crate::drivers::virtio::reg_w(b, crate::drivers::virtio::R_QUEUE_NOTIFY, 0);
        // Poll for completion (reuse kick logic but with larger resp).
        let used_idx_ptr = core::ptr::addr_of!((*u).idx);
        let mut spins = 0u64;
        loop {
            fence(Ordering::SeqCst);
            let used = read_volatile(used_idx_ptr);
            if used != *lu {
                *lu = used;
                let resp = read_volatile(resp_buf as *const u32);
                if resp == 0x1101 {
                    // VIRTIO_GPU_RESP_OK_DISPLAY_INFO = 0x1101
                    return Ok(());
                }
                if resp == R_OK {
                    return Ok(());
                }
                crate::kwrn!(
                    "virtio-gpu",
                    "get_display_info resp=0x%x",
                    onyx_core::fmt::Arg::from(resp)
                );
                return Err(Errno::Io);
            }
            spins += 1;
            if spins == 500_000 {
                crate::kwrn!(
                    "virtio-gpu",
                    "get_display_info spins=500k avail=0x%x used=0x%x",
                    onyx_core::fmt::Arg::from(read_volatile(core::ptr::addr_of!((*a).idx)) as u32),
                    onyx_core::fmt::Arg::from(read_volatile(used_idx_ptr) as u32)
                );
            }
            if spins > 5_000_000 {
                crate::kwrn!("virtio-gpu", "get_display_info timeout");
                return Err(Errno::Io);
            }
        }
    }
}

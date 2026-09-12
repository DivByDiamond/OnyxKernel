//! virtio-gpu 2D control commands: create a resource, attach a backing
//! buffer, set the scanout, and query display info.
use super::super::{C_RESOURCE_CREATE_2D, C_SET_SCANOUT, Create2D, GpuCtrlHdr, SetScanout};
use super::{C_ATTACH, C_GET_DISPLAY_INFO, GPU_QSIZE, R_OK, send_cmd};
use crate::drivers::virtio::{VQ_DESC_F_NEXT, VQ_DESC_F_WRITE, VqAvail, VqDesc, VqUsed};
use crate::mm::pmm;
use onyx_core::errno::{Errno, KResult};

/// # Safety
///
/// Queue pointers per `super::queue::kick`; the command struct must outlive the synchronous kick (true for the stack local).
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
/// `rid`/`pa`/`len` must describe a device resource backed by `len` bytes at physical `pa`; queue pointers per `super::queue::kick`.
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
        super::queue::kick(d, a, u, lu, b, rp)
    }
}

/// # Safety
///
/// Queue pointers per `super::queue::kick`; the command struct must outlive the synchronous kick (true for the stack local).
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

/// # Safety
///
/// Queue pointers per `super::queue::kick`.
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

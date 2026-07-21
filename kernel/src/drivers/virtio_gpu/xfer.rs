use super::{Create2D, GpuCtrlHdr, SetScanout, C_RESOURCE_CREATE_2D, C_SET_SCANOUT};
use crate::drivers::virtio::{
    reg_w, VqAvail, VqDesc, VqUsed, R_QUEUE_AVAIL_HIGH, R_QUEUE_AVAIL_LOW, R_QUEUE_DESC_HIGH,
    R_QUEUE_DESC_LOW, R_QUEUE_ENABLE, R_QUEUE_NOTIFY, R_QUEUE_NUM, R_QUEUE_SEL, R_QUEUE_USED_HIGH,
    R_QUEUE_USED_LOW, VIRTQ_SIZE, VQ_DESC_F_NEXT, VQ_DESC_F_WRITE,
};
use crate::mm::pmm;
use onyx_core::errno::{Errno, KResult};

const R_OK: u32 = 0x1100;
const C_ATTACH: u32 = 0x106;

pub unsafe fn setup_queue(
    d: *mut *mut VqDesc,
    a: *mut *mut VqAvail,
    u: *mut *mut VqUsed,
    b: usize,
) -> KResult<()> {
    reg_w(b, R_QUEUE_SEL, 0);
    reg_w(b, R_QUEUE_NUM, VIRTQ_SIZE as u32);
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
    reg_w(b, R_QUEUE_ENABLE, 1);
    Ok(())
}

unsafe fn kick(
    d: *mut VqDesc,
    a: *mut VqAvail,
    u: *mut VqUsed,
    lu: *mut u16,
    b: usize,
    rp: usize,
) -> KResult<()> {
    let i = (*a).idx as usize % VIRTQ_SIZE;
    *&mut *d.add((i + 1) % VIRTQ_SIZE) = VqDesc {
        addr: rp as u64,
        len: 16,
        flags: VQ_DESC_F_WRITE,
        next: 0,
    };
    let idx = (*a).idx;
    (*a).ring[idx as usize % VIRTQ_SIZE] = i as u16;
    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
    (*a).idx = idx.wrapping_add(1);
    reg_w(b, R_QUEUE_NOTIFY, 0);
    let mut t = 1_000_000u32;
    while t > 0 {
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        if (*u).idx != *lu {
            *lu = (*u).idx;
            if (*(rp as *const GpuCtrlHdr)).hdr_type == R_OK {
                return Ok(());
            }
            return Err(Errno::Io);
        }
        t -= 1;
    }
    Err(Errno::Io)
}

pub unsafe fn send_cmd(
    d: *mut VqDesc,
    a: *mut VqAvail,
    u: *mut VqUsed,
    lu: *mut u16,
    b: usize,
    cmd: *mut u8,
    len: u32,
) -> KResult<()> {
    let rp = pmm::alloc_zero()? as usize;
    let i = (*a).idx as usize % VIRTQ_SIZE;
    *&mut *d.add(i) = VqDesc {
        addr: cmd as u64,
        len,
        flags: VQ_DESC_F_NEXT,
        next: ((i + 1) % VIRTQ_SIZE) as u16,
    };
    kick(d, a, u, lu, b, rp)
}

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
    let c = Create2D {
        hdr: GpuCtrlHdr {
            hdr_type: C_RESOURCE_CREATE_2D,
            flags: 0,
            fence_id: 0,
            ctx_id: 0,
            padding: 0,
        },
        resource_id: rid,
        format: 2,
        width: w,
        height: h,
    };
    send_cmd(
        d,
        a,
        u,
        lu,
        b,
        &c as *const _ as *mut u8,
        core::mem::size_of::<Create2D>() as u32,
    )
}

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
    let rp = pmm::alloc_zero()? as usize;
    let i = (*a).idx as usize % VIRTQ_SIZE;
    *&mut *d.add(i) = VqDesc {
        addr: buf as u64,
        len: 44,
        flags: VQ_DESC_F_NEXT,
        next: ((i + 1) % VIRTQ_SIZE) as u16,
    };
    *&mut *d.add((i + 1) % VIRTQ_SIZE) = VqDesc {
        addr: rp as u64,
        len: 16,
        flags: VQ_DESC_F_WRITE,
        next: 0,
    };
    kick(d, a, u, lu, b, rp)
}

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
        resource_id: rid,
    };
    send_cmd(
        d,
        a,
        u,
        lu,
        b,
        &c as *const _ as *mut u8,
        core::mem::size_of::<SetScanout>() as u32,
    )
}

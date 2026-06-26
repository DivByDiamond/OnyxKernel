//! virtio-net frame I/O — polling receive and blocking send.
use super::{push_avail, G_NET, HDR_LEN, NET_MTU, RX_DESCS};
use crate::drivers::virtio::{R_QUEUE_NOTIFY, VQ_DESC_F_NEXT};
use crate::mm::pmm;
use core::ptr;
use onyx_core::errno::{Errno, KResult};

/// Poll for a received Ethernet frame. Copies up to `out.len()` bytes.
/// Returns the number of bytes received, or `Err(NoEnt)` if no frame is ready.
pub fn recv_into(out: &mut [u8]) -> KResult<usize> {
    unsafe {
        let used_idx = ptr::read_volatile(ptr::addr_of!((*G_NET.used).idx));
        if used_idx == G_NET.last_used {
            return Err(Errno::NoEnt);
        }
        let slot = (G_NET.last_used as usize) % RX_DESCS;
        G_NET.last_used = used_idx;
        let elem = ptr::read_volatile(ptr::addr_of!((*G_NET.used).ring[slot]));
        let buf_idx = (elem.idx as usize) % RX_DESCS;
        let frame_len = (elem.len as usize)
            .saturating_sub(HDR_LEN)
            .min(out.len());
        let src = G_NET.rx_bufs[buf_idx].add(HDR_LEN);
        ptr::copy_nonoverlapping(src, out.as_mut_ptr(), frame_len);
        push_avail(buf_idx);
        Ok(frame_len)
    }
}

/// Send a raw Ethernet frame. Blocks until the device consumes the buffer.
pub fn send(frame: &[u8]) -> KResult<()> {
    if frame.is_empty() || frame.len() > NET_MTU {
        return Err(Errno::Inval);
    }
    unsafe {
        let hdr_pa = pmm::alloc_zero()? as *mut u8;
        let frame_pa = pmm::alloc_zero()? as *mut u8;
        ptr::copy_nonoverlapping(frame.as_ptr(), frame_pa, frame.len());
        // Two descriptors chained: header (read-only) + frame (read-only).
        (*G_NET.desc.add(0)) = crate::drivers::virtio::VqDesc {
            addr: hdr_pa as u64,
            len: HDR_LEN as u32,
            flags: VQ_DESC_F_NEXT,
            next: 1,
        };
        (*G_NET.desc.add(1)) = crate::drivers::virtio::VqDesc {
            addr: frame_pa as u64,
            len: frame.len() as u32,
            flags: 0,
            next: 0,
        };
        push_avail(0);
        let base = G_NET.base;
        crate::drivers::virtio::reg_w(base, R_QUEUE_NOTIFY, 0);
        let last = ptr::read_volatile(ptr::addr_of!((*G_NET.used).idx));
        // Wait for one new used entry.
        loop {
            let cur = ptr::read_volatile(ptr::addr_of!((*G_NET.used).idx));
            if cur != last {
                let _ = last;
                break;
            }
        }
        pmm::free(hdr_pa as u64);
        pmm::free(frame_pa as u64);
        Ok(())
    }
}

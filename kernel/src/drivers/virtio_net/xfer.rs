//! virtio-net frame I/O — polling receive and blocking send.
use super::{G_NET, HDR_LEN, NET_MTU, RX_DESCS, push_avail};
use crate::drivers::virtio::{R_QUEUE_NOTIFY, VQ_DESC_F_NEXT};
use crate::mm::pmm;
use core::ptr;
use onyx_core::errno::{Errno, KResult};

/// Poll for a received Ethernet frame. Copies up to `out.len()` bytes.
/// Returns the number of bytes received, or `Err(NoEnt)` if no frame is ready.
pub fn recv_into(out: &mut [u8]) -> KResult<usize> {
    // SAFETY: valid only after init() completed (guarded here); slot/buf_idx masked % RX_DESCS keep used-ring and rx_bufs accesses in bounds; the device only writes HDR_LEN+NET_MTU-byte RX buffers and the copy length is clamped to out.len().
    unsafe {
        if G_NET.base == 0 || G_NET.rx_avail.is_null() || G_NET.rx_used.is_null() {
            return Err(Errno::Io);
        }
        let used_idx = ptr::read_volatile(ptr::addr_of!((*G_NET.rx_used).idx));
        if used_idx == G_NET.rx_last_used {
            return Err(Errno::NoEnt);
        }
        let slot = (G_NET.rx_last_used as usize) % RX_DESCS;
        G_NET.rx_last_used = used_idx;
        let elem = ptr::read_volatile(ptr::addr_of!((*G_NET.rx_used).ring[slot]));
        let buf_idx = (elem.idx as usize) % RX_DESCS;
        let frame_len = (elem.len as usize).saturating_sub(HDR_LEN).min(out.len());
        let src = G_NET.rx_bufs[buf_idx].add(HDR_LEN);
        ptr::copy_nonoverlapping(src, out.as_mut_ptr(), frame_len);
        push_avail(buf_idx, true);
        Ok(frame_len)
    }
}

/// Send a raw Ethernet frame. Blocks until the device consumes the buffer.
pub fn send(frame: &[u8]) -> KResult<()> {
    if frame.is_empty() || frame.len() > NET_MTU {
        return Err(Errno::Inval);
    }
    // SAFETY: valid only after init() completed (guarded below); desc slots 0/1 < VIRTQ_SIZE reference fresh PMM pages; frame.len() <= NET_MTU checked above; buffers freed only after used idx advances (leaked on timeout to avoid device-side use-after-free).
    unsafe {
        // Never dereference a half-initialized device: init() exposes the
        // global only after the queues are up, but keep the guard as defense
        // in depth so a stray caller can't fault the kernel on a NULL pointer.
        if G_NET.base == 0
            || G_NET.tx_desc.is_null()
            || G_NET.tx_avail.is_null()
            || G_NET.tx_used.is_null()
        {
            return Err(Errno::Io);
        }
        let hdr_pa = pmm::alloc_zero()? as *mut u8;
        let frame_pa = pmm::alloc_zero()? as *mut u8;
        ptr::copy_nonoverlapping(frame.as_ptr(), frame_pa, frame.len());
        // Two descriptors chained: header (read-only) + frame (read-only).
        (*G_NET.tx_desc.add(0)) = crate::drivers::virtio::VqDesc {
            addr: hdr_pa as u64,
            len: HDR_LEN as u32,
            flags: VQ_DESC_F_NEXT,
            next: 1,
        };
        (*G_NET.tx_desc.add(1)) = crate::drivers::virtio::VqDesc {
            addr: frame_pa as u64,
            len: frame.len() as u32,
            flags: 0,
            next: 0,
        };
        push_avail(0, false);
        let base = G_NET.base;
        // Queue index 1 = transmitq. Notifying 0 here (the original bug)
        // tells the device "the RX queue has new buffers", which is not a
        // send — and it targets the RX rings anyway, since there was no
        // separate TX queue before this fix.
        crate::drivers::virtio::reg_w(base, R_QUEUE_NOTIFY, 1);
        let last = ptr::read_volatile(ptr::addr_of!((*G_NET.tx_used).idx));
        // Wait for one new used entry. sedna's VirtIONetworkDevice only marks
        // the TX descriptor used when the OC2R network stack pulls the frame
        // on its own tick (async w.r.t. the guest), so a fixed small spin
        // bound would fail a live link. Bound by uptime instead, with a hard
        // spin cap as a fail-safe in case the timer does not advance.
        let deadline = crate::srv::timer::uptime_us().wrapping_add(250_000);
        let mut spins = 0u64;
        loop {
            let cur = ptr::read_volatile(ptr::addr_of!((*G_NET.tx_used).idx));
            if cur != last {
                break;
            }
            spins += 1;
            if spins >= 10_000_000 || crate::srv::timer::uptime_us() >= deadline {
                // Do NOT free the buffers here: sedna's network stack pulls TX
                // chains asynchronously on its own tick and may still consume
                // the (stale) descriptors, so freeing would be a use-after-free.
                // Two leaked pages per failed send is fine — this only happens
                // when no network peer is present (DHCP falls back and moves on).
                return Err(Errno::Io);
            }
        }
        pmm::free(hdr_pa as u64);
        pmm::free(frame_pa as u64);
        Ok(())
    }
}

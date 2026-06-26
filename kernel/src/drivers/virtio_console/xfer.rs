//! virtio-console RX/TX — polled receive, synchronous transmit.
use super::{push, G_CON};
use crate::drivers::virtio::{R_QUEUE_NOTIFY, VqDesc};
use crate::mm::pmm;
use core::ptr;
use onyx_core::errno::{Errno, KResult};

/// Poll for one received byte, or `None` if no data is available.
pub fn getc() -> Option<u8> {
    unsafe {
        let used_idx = ptr::read_volatile(ptr::addr_of!((*G_CON.rx_used).idx));
        if used_idx == G_CON.rx_last {
            return None;
        }
        G_CON.rx_last = used_idx;
        let byte = *G_CON.rx_buf;
        // Recycle the RX buffer so we can receive the next byte.
        push(0, true);
        Some(byte)
    }
}

/// Send a single byte synchronously. Allocates a temporary page for the
/// TX payload — virtio-console expects the descriptor address to be a
/// physical address, so we use PMM directly.
pub fn putc(b: u8) -> KResult<()> {
    unsafe {
        let buf_pa = pmm::alloc_zero()? as *mut u8;
        *buf_pa = b;
        (*G_CON.tx_desc.add(0)) = VqDesc {
            addr: buf_pa as u64,
            len: 1,
            flags: 0,
            next: 0,
        };
        push(0, false);
        reg_w(G_CON.base, R_QUEUE_NOTIFY, 1);
        let used_idx = ptr::read_volatile(ptr::addr_of!((*G_CON.tx_used).idx));
        #[allow(clippy::while_immutable_condition)]
        while ptr::read_volatile(ptr::addr_of!((*G_CON.tx_used).idx)) == used_idx {}
        pmm::free(buf_pa as u64);
        Ok(())
    }
}

/// Send a string byte-by-byte.
pub fn puts(s: &str) {
    for &b in s.as_bytes() {
        let _ = putc(b);
    }
}

use crate::drivers::virtio::reg_w;

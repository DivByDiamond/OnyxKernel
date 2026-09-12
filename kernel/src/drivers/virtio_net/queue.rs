//! virtio-net avail-ring bookkeeping shared by init and frame I/O.
use super::G_NET;
use crate::drivers::virtio::VIRTQ_SIZE;
use core::ptr;

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

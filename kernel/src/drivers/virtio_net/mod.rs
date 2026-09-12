//! virtio-net MMIO driver — root module.
//!
//! Owns the device struct and global state. Probe/init sequence lives in
//! `init.rs`, avail-ring bookkeeping in `queue.rs`, and frame I/O in
//! `xfer.rs`.
use crate::drivers::virtio::{VqAvail, VqDesc, VqUsed};
use core::ptr;

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

pub fn mac() -> [u8; 6] {
    // SAFETY: copy of the 6-byte MAC written during single-threaded boot init; kernel code never runs with SIE set (see crate::sync).
    unsafe { G_NET.mac }
}

mod init;
mod queue;
pub mod xfer;

pub use init::{init, probe};
pub(crate) use queue::push_avail;
pub use xfer::{recv_into, send};

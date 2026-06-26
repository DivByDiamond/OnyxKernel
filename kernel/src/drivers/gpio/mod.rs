//! SiFive GPIO controller — register constants, state, probe/init.
//!
//! QEMU virt exposes a SiFive GPIO at 0x10060000 with 32 pins. The driver
//! keeps a tiny dispatch table mapping pin → handler so device drivers
//! (LEDs, buttons, SD-card-CD) can register for edge interrupts without
//! touching PLIC directly. Pin I/O API lives in `ops.rs`.
pub use self::ops::{
    dispatch, on_edge, read, set_invert, set_input, set_output, toggle, write, PinHandler,
    PinSlot,
};
use crate::arch::mmio::Mmio;

pub const GPIO_BASE: usize = 0x1006_0000;
pub const N_PINS: usize = 32;

// SiFive GPIO register offsets (spec §3.1)
pub(crate) const R_INPUT_VAL: u32 = 0x00;
pub(crate) const R_INPUT_EN: u32 = 0x04;
pub(crate) const R_OUTPUT_EN: u32 = 0x08;
pub(crate) const R_OUTPUT_VAL: u32 = 0x0C;
pub(crate) const R_RISE_IE: u32 = 0x18;
pub(crate) const R_RISE_IP: u32 = 0x1C;
pub(crate) const R_FALL_IE: u32 = 0x20;
pub(crate) const R_FALL_IP: u32 = 0x24;
pub(crate) const _R_HIGH_IE: u32 = 0x28;
pub(crate) const _R_HIGH_IP: u32 = 0x2C;
pub(crate) const _R_LOW_IE: u32 = 0x30;
pub(crate) const _R_LOW_IP: u32 = 0x34;
pub(crate) const R_OUT_XOR: u32 = 0x40;

pub(crate) static mut G_PINS: [PinSlot; N_PINS] = [PinSlot { handler: None }; N_PINS];
pub(crate) static mut G_BASE: usize = GPIO_BASE;

#[inline]
pub(crate) unsafe fn rd(off: u32) -> u32 {
    Mmio::<u32>::at(G_BASE + off as usize).read()
}

#[inline]
pub(crate) unsafe fn wr(off: u32, v: u32) {
    Mmio::<u32>::at(G_BASE + off as usize).write(v);
}

/// Initialise the controller at the given base address. Disables and
/// clears all edge interrupts so drivers can register cleanly.
pub unsafe fn init(base: usize) {
    G_BASE = base;
    wr(R_RISE_IE, 0);
    wr(R_FALL_IE, 0);
    wr(_R_HIGH_IE, 0);
    wr(_R_LOW_IE, 0);
    wr(R_RISE_IP, !0);
    wr(R_FALL_IP, !0);
    wr(_R_HIGH_IP, !0);
    wr(_R_LOW_IP, !0);
}

pub mod ops;

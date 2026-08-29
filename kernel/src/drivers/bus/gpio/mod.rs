//! SiFive GPIO controller — register constants, state, probe/init.
//!
//! QEMU virt exposes a SiFive GPIO at 0x10060000 with 32 pins. The driver
//! keeps a tiny dispatch table mapping pin → handler so device drivers
//! (LEDs, buttons, SD-card-CD) can register for edge interrupts without
//! touching PLIC directly. Pin I/O API lives in `ops.rs`.
pub use self::ops::{
    PinHandler, PinSlot, dispatch, on_edge, read, set_input, set_invert, set_output, toggle, write,
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

/// # Safety
/// G_BASE must hold a valid SiFive GPIO MMIO base; `off` must be within the controller register file.
#[inline]
pub(crate) unsafe fn rd(off: u32) -> u32 {
    // SAFETY: G_BASE was set by init() from a probed/validated GPIO base
    // (QEMU virt fixed 0x1006_0000 or FDT-derived), identity-mapped at boot;
    // off is a datasheet register offset defined in this file.
    unsafe { Mmio::<u32>::at(G_BASE + off as usize).read() }
}

/// # Safety
/// G_BASE must hold a valid GPIO MMIO base; `off` must be within the register file.
#[inline]
pub(crate) unsafe fn wr(off: u32, v: u32) {
    // SAFETY: same contract as `rd`; off is a controller register offset defined in this file.
    unsafe {
        Mmio::<u32>::at(G_BASE + off as usize).write(v);
    }
}

/// Initialise the controller at the given base address. Disables and
/// clears all edge interrupts so drivers can register cleanly.
/// # Safety
/// `base` must be a valid SiFive GPIO MMIO base, identity-mapped; no concurrent GPIO use while rebinding G_BASE.
pub unsafe fn init(base: usize) {
    // SAFETY: G_BASE is rebound to the caller-provided probed base on the
    // single-threaded init path; register offsets are per spec §3.1.
    unsafe {
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
}

pub mod ops;

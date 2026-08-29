//! SiFive I2C master — register constants, state, probe/init.
//!
//! Implements a minimal subset of the SiFive I2C controller (QEMU virt
//! 0x10030000). Bus primitives (start/stop/read/write) live in `xfer.rs`;
//! high-level scan/read/write live in `ops.rs`.
pub use self::ops::{read, scan, write};
pub use self::xfer::{read_byte, start, wait_not_busy, wait_tip, write_byte};

use crate::arch::mmio::Mmio;

pub const I2C_BASE: usize = 0x1003_0000;
pub const TIMEOUT: u32 = 1_000_000;

pub(crate) const R_PRESCALE_LO: u32 = 0x00;
pub(crate) const R_PRESCALE_HI: u32 = 0x04;
pub(crate) const R_CONTROL: u32 = 0x08;
pub(crate) const R_TXRX: u32 = 0x0C;
pub(crate) const R_CMD_STATUS: u32 = 0x10;

pub(crate) const C_ENABLE: u32 = 1 << 7;
pub(crate) const STA: u32 = 1 << 7;
pub(crate) const STO: u32 = 1 << 6;
pub(crate) const RD: u32 = 1 << 5;
pub(crate) const WR: u32 = 1 << 4;
pub(crate) const ACK: u32 = 1 << 3;
pub(crate) const _IACK: u32 = 1 << 0;

pub(crate) const S_RXACK: u32 = 1 << 7;
pub(crate) const S_BUSY: u32 = 1 << 5;
pub(crate) const S_TIP: u32 = 1 << 1;
pub(crate) const _S_IF: u32 = 1 << 0;

pub(crate) static mut G_BASE: usize = I2C_BASE;

/// # Safety
/// G_BASE must hold a valid SiFive I2C MMIO base; `off` must be within the controller register file.
#[inline]
pub(crate) unsafe fn rd(off: u32) -> u32 {
    // SAFETY: G_BASE was set by init() from a probed/validated I2C base
    // (QEMU virt fixed 0x1003_0000 or FDT-derived), identity-mapped at boot;
    // off is a datasheet register offset defined in this file.
    unsafe { Mmio::<u32>::at(G_BASE + off as usize).read() & 0xFF }
}

/// # Safety
/// G_BASE must hold a valid I2C MMIO base; `off` must be within the register file.
#[inline]
pub(crate) unsafe fn wr(off: u32, v: u32) {
    // SAFETY: same contract as `rd`; off is a controller register offset
    // defined in this file.
    unsafe {
        Mmio::<u32>::at(G_BASE + off as usize).write(v & 0xFF);
    }
}

/// Initialise the controller. `prescale` = (clk / (5 * i2c_hz)) - 1.
/// For 100 kHz on a 50 MHz peripheral clock, prescale ≈ 99.
/// # Safety
/// `base` must be a valid SiFive I2C MMIO base, identity-mapped; no concurrent I2C use while rebinding G_BASE.
pub unsafe fn init(base: usize, prescale: u16) {
    // SAFETY: G_BASE is rebound to the caller-provided probed base on the
    // single-threaded init path; register offsets are per the constants in
    // this file.
    unsafe {
        G_BASE = base;
        wr(R_CONTROL, 0);
        wr(R_PRESCALE_LO, prescale as u32);
        wr(R_PRESCALE_HI, (prescale >> 8) as u32);
        wr(R_CONTROL, C_ENABLE);
    }
}

pub mod edid;
pub mod ops;
pub mod xfer;

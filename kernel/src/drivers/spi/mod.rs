//! SiFive SPI master — register constants, state, probe/init.
//!
//! QEMU virt exposes a SiFive SPI controller at 0x10040000 with up to 4
//! chip-select lines. Bus primitives live in `xfer.rs`; high-level
//! read/write live in `ops.rs`.
pub use self::ops::{read, write};
pub use self::xfer::{select, release, transfer};

use crate::arch::mmio::Mmio;

pub const SPI_BASE: usize = 0x1004_0000;
pub const TIMEOUT: u32 = 1_000_000;
pub const MAX_CS: u8 = 4;

pub(crate) const R_SCKDIV: u32 = 0x00;
pub(crate) const _R_SCKMODE: u32 = 0x04;
pub(crate) const R_CSID: u32 = 0x10;
pub(crate) const R_CSDEF: u32 = 0x14;
pub(crate) const R_CSMODE: u32 = 0x18;
pub(crate) const _R_DELAY0: u32 = 0x28;
pub(crate) const _R_DELAY1: u32 = 0x2C;
pub(crate) const R_FMT: u32 = 0x40;
pub(crate) const R_TXDATA: u32 = 0x48;
pub(crate) const R_RXDATA: u32 = 0x4C;
pub(crate) const R_TXMARK: u32 = 0x50;
pub(crate) const R_RXMARK: u32 = 0x54;
pub(crate) const R_FCTRL: u32 = 0x60;
pub(crate) const _R_FFMT: u32 = 0x64;

pub(crate) const TX_FULL: u32 = 1 << 31;
pub(crate) const RX_EMPTY: u32 = 1 << 31;

pub(crate) const CSMODE_AUTO: u32 = 0;
pub(crate) const CSMODE_HOLD: u32 = 2;
pub(crate) const _CSMODE_OFF: u32 = 3;

pub(crate) const FMT_PROTO_SPI: u32 = 0 << 0;
pub(crate) const FMT_ENDIAN_MSB: u32 = 0 << 2;
pub(crate) const _FMT_DIR_RX: u32 = 1 << 3;
pub(crate) const FMT_LEN_8: u32 = 8 << 16;

pub(crate) static mut G_BASE: usize = SPI_BASE;

#[inline]
pub(crate) unsafe fn rd(off: u32) -> u32 {
    Mmio::<u32>::at(G_BASE + off as usize).read()
}

#[inline]
pub(crate) unsafe fn wr(off: u32, v: u32) {
    Mmio::<u32>::at(G_BASE + off as usize).write(v);
}

/// Initialise the controller. `sckdiv` = (clk / (2 * spi_hz)) - 1.
/// `cs` is the default chip-select line (0..MAX_CS).
pub unsafe fn init(base: usize, sckdiv: u32, cs: u8) {
    G_BASE = base;
    wr(R_SCKDIV, sckdiv);
    wr(R_CSID, cs as u32);
    wr(R_CSDEF, 1 << cs);
    wr(R_CSMODE, CSMODE_AUTO);
    wr(R_FMT, FMT_PROTO_SPI | FMT_ENDIAN_MSB | FMT_LEN_8);
    wr(R_FCTRL, 0);
    wr(R_TXMARK, 1);
    wr(R_RXMARK, 0);
}

pub mod xfer;
pub mod ops;

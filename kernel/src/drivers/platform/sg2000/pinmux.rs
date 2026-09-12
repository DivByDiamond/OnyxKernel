//! SG2000 FMUX pinmux (TRM §8.1.2 "FMUX 寄存器描述").
//!
//! Only the header pins actually usable for SPI on the Duo S 40-pin
//! header are wired here (the `MUX_SPI1_*` group); other SPI instances
//! (SPI0/2/3) and the dedicated MIPI Tx pads use different FMUX
//! registers not exercised by this driver and are intentionally left
//! out rather than guessed.
use crate::arch::mmio::Mmio;

const MUX_SPI1_MISO: usize = 0x0300_1114;
const MUX_SPI1_MOSI: usize = 0x0300_1118;
const MUX_SPI1_CS: usize = 0x0300_111C;
const MUX_SPI1_SCK: usize = 0x0300_1120;

/// FMUX function-select value routing each `MUX_SPI1_*` pin to its
/// `SPI1_*` signal (TRM §8.1.2, function-select tables for each register:
/// `6 : SPI1_SDI` / `SPI1_SDO` / `SPI1_CS_X` / `SPI1_SCK`).
const FUNC_SPI1: u32 = 6;

/// # Safety
/// `reg` must be a real FMUX register address in the `0x0300_1xxx`
/// window; must run during single-threaded board bring-up (SIE=0).
unsafe fn set_fmux(reg: usize, func: u32) {
    // SAFETY: reg is a datasheet FMUX register address in the documented PINMUX window (0x0300_1000..0x0300_1FFF), identity-mapped at boot.
    unsafe { Mmio::<u32>::at(reg).write(func) };
}

/// Route the Duo S header's SCK/MOSI/MISO/CS pins to SPI1. Must be
/// called before `spi::dw::init` for SPI1 on SG2000 hardware — the pins
/// default to GPIO (TRM reset value `3`), not SPI, on power-up.
/// # Safety
/// Must run once during single-threaded board bring-up (SIE=0).
pub unsafe fn spi1_pins() {
    // SAFETY: set_fmux's contract is upheld by this function's own contract; all four addresses are FMUX registers documented above.
    unsafe {
        set_fmux(MUX_SPI1_SCK, FUNC_SPI1);
        set_fmux(MUX_SPI1_MOSI, FUNC_SPI1);
        set_fmux(MUX_SPI1_MISO, FUNC_SPI1);
        set_fmux(MUX_SPI1_CS, FUNC_SPI1);
    }
}

//! SG2000 CLK_DIV CRG clock gating (TRM §6.10 "CLK_DIV CRG 寄存器描述").
//!
//! Peripherals power up with most gates already enabled at reset (TRM
//! reset value `0x1` per bit), but SPI/DSI must not be touched by driver
//! code without confirming the gate is on — a future reset-value change
//! or a shared clock domain being gated for another peripheral would
//! otherwise cause silent MMIO hangs.
use super::CRU_BASE;
use crate::arch::mmio::Mmio;

const R_CLK_EN_2: usize = 0x008;
const R_CLK_EN_3: usize = 0x00C;

/// `clk_en_2` bit 31 = `clk_dsi_mac_vip` (TRM §6.10, `clk_en_2` table).
const BIT_DSI_MAC: u32 = 1 << 31;
/// `clk_en_3` bit 6 = `clk_spi` (TRM §6.10, `clk_en_3` table).
const BIT_SPI: u32 = 1 << 6;

/// # Safety
/// `CRU_BASE` must be identity-mapped; must run during single-threaded
/// board bring-up (SIE=0) since it read-modify-writes a shared gate
/// register also touched by other peripherals' init paths.
unsafe fn set_bit(off: usize, bit: u32) {
    // SAFETY: off is a datasheet register offset within the CRU's documented window; CRU_BASE is identity-mapped at boot per platform init.
    unsafe {
        let r = Mmio::<u32>::at(CRU_BASE + off);
        let v = r.read();
        r.write(v | bit);
    }
}

/// Enable the SPI peripheral clock gate. Must be called before any
/// `spi::dw::init` on SG2000 hardware — the controller does not respond
/// to MMIO while its clock is gated.
/// # Safety
/// Must run once during single-threaded board bring-up (SIE=0).
pub unsafe fn enable_spi() {
    // SAFETY: set_bit's contract is upheld by this function's own contract.
    unsafe { set_bit(R_CLK_EN_3, BIT_SPI) };
}

/// Enable the DSI MAC peripheral clock gate. Must be called before any
/// `mipi_dsi::init` on SG2000 hardware.
/// # Safety
/// Must run once during single-threaded board bring-up (SIE=0).
pub unsafe fn enable_dsi() {
    // SAFETY: set_bit's contract is upheld by this function's own contract.
    unsafe { set_bit(R_CLK_EN_2, BIT_DSI_MAC) };
}

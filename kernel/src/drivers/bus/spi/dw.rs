//! Synopsys DesignWare APB SSI (DW_apb_ssi) master — used by the SG2000
//! SoC on the Milk-V Duo S for the general-purpose SPI peripherals
//! (distinct from the SiFive controller in `mod.rs`, which targets QEMU
//! virt). Register map per the public DW_apb_ssi databook: CTRLR0/CTRLR1,
//! SSIENR, SER, BAUDR, FIFO threshold/level regs, SR, and an indexed DR
//! FIFO window.
//!
//! `base` is caller-supplied (from FDT/board config) rather than a fixed
//! constant: the exact SPI peripheral MMIO base on SG2000 depends on
//! which of the SoC's SSI instances is wired to the header, and must be
//! confirmed against the SG2000 TRM before use on real hardware.
use crate::arch::mmio::Mmio;
use crate::drivers::platform::sg2000;
use onyx_core::errno::{Errno, KResult};

const R_CTRLR0: u32 = 0x00;
const R_CTRLR1: u32 = 0x04;
const R_SSIENR: u32 = 0x08;
const R_SER: u32 = 0x10;
const R_BAUDR: u32 = 0x14;
const R_TXFTLR: u32 = 0x18;
const R_RXFTLR: u32 = 0x1C;
const R_SR: u32 = 0x28;
const R_DR0: u32 = 0x60;

const SR_BUSY: u32 = 1 << 0;
const SR_TFNF: u32 = 1 << 1; // TX FIFO not full
const SR_RFNE: u32 = 1 << 3; // RX FIFO not empty

const CTRLR0_DFS_8BIT: u32 = 7; // bits[3:0], (frame_size - 1)
const CTRLR0_TMOD_TXRX: u32 = 0 << 8; // bits[9:8], full-duplex

const TIMEOUT: u32 = 1_000_000;
pub const MAX_CS: u8 = 4;

static mut G_BASE: usize = 0;

/// # Safety
/// `G_BASE` must hold a valid DW_apb_ssi MMIO base set by [`init`]; `off`
/// must be a register offset defined in this file.
#[inline]
unsafe fn rd(off: u32) -> u32 {
    // SAFETY: G_BASE was set by init() from a caller-provided, board-validated base; off is a databook register offset.
    unsafe { Mmio::<u32>::at(G_BASE + off as usize).read() }
}

/// # Safety
/// Same contract as [`rd`].
#[inline]
unsafe fn wr(off: u32, v: u32) {
    // SAFETY: same contract as rd(); off is a databook register offset defined in this file.
    unsafe {
        Mmio::<u32>::at(G_BASE + off as usize).write(v);
    }
}

/// Initialise a DW_apb_ssi instance for 8-bit full-duplex mode.
/// `sckdiv` is the SSI_CLK divider (even value, `ssi_clk = ssi_ic_clk / sckdiv`).
/// `cs` is the default chip-select line (0..MAX_CS).
///
/// # Safety
/// `base` must be a real DW_apb_ssi MMIO base, identity-mapped, with the
/// peripheral's clock/reset already enabled by platform init; no
/// concurrent SPI use while rebinding `G_BASE`.
pub unsafe fn init(base: usize, sckdiv: u32, cs: u8) -> KResult<()> {
    if base == 0 || cs >= MAX_CS {
        return Err(Errno::Inval);
    }
    // SAFETY: G_BASE is rebound to the caller-provided base on the
    // single-threaded init path; SSIENR is cleared before reconfiguring
    // per the databook ("disable before changing CTRLR0/BAUDR").
    unsafe {
        G_BASE = base;
        wr(R_SSIENR, 0);
        wr(R_CTRLR0, CTRLR0_TMOD_TXRX | CTRLR0_DFS_8BIT);
        wr(R_CTRLR1, 0);
        wr(R_BAUDR, sckdiv);
        wr(R_TXFTLR, 0);
        wr(R_RXFTLR, 0);
        wr(R_SER, 1 << cs);
        wr(R_SSIENR, 1);
    }
    Ok(())
}

/// Bring up SPI1 on the Milk-V Duo S 40-pin header: gate on `clk_spi`,
/// mux the header's SCK/MOSI/MISO/CS pins to SPI1 (TRM §8.1.2), then
/// initialise the SPI1 controller at its TRM-confirmed base
/// (`sg2000::SPI_BASES[1]` == `0x0419_0000`). `sckdiv`/`cs` as in [`init`].
///
/// # Safety
/// Must run once during single-threaded board bring-up (SIE=0); no
/// concurrent SPI use while rebinding `G_BASE`.
pub unsafe fn init_sg2000_spi1(sckdiv: u32, cs: u8) -> KResult<()> {
    // SAFETY: enable_spi/spi1_pins/init each uphold their own single-threaded-bring-up contract, satisfied by this function's own contract.
    unsafe {
        sg2000::enable_spi();
        sg2000::spi1_pins();
        init(sg2000::SPI_BASES[1], sckdiv, cs)
    }
}

/// Full-duplex transfer of `len` bytes. `tx`/`rx` must be equal-length.
pub fn transfer(tx: &[u8], rx: &mut [u8]) -> KResult<()> {
    if tx.len() != rx.len() || tx.is_empty() {
        return Err(Errno::Inval);
    }
    // SAFETY: rd()/wr() access the FIFO/status registers inside the
    // controller MMIO window bound by init(); rx[i] is bounds-checked by
    // the equal-length contract checked above.
    unsafe {
        for (i, &b) in tx.iter().enumerate() {
            let mut t = TIMEOUT;
            while rd(R_SR) & SR_TFNF == 0 {
                t -= 1;
                if t == 0 {
                    return Err(Errno::Io);
                }
            }
            wr(R_DR0, b as u32);
            let mut t2 = TIMEOUT;
            while rd(R_SR) & SR_RFNE == 0 {
                t2 -= 1;
                if t2 == 0 {
                    return Err(Errno::Io);
                }
            }
            rx[i] = rd(R_DR0) as u8;
        }
        let mut t3 = TIMEOUT;
        while rd(R_SR) & SR_BUSY != 0 {
            t3 -= 1;
            if t3 == 0 {
                return Err(Errno::Io);
            }
        }
        Ok(())
    }
}

/// Change the active chip-select line without a full re-init.
pub fn select(cs: u8) -> KResult<()> {
    if cs >= MAX_CS {
        return Err(Errno::Inval);
    }
    // SAFETY: wr() accesses SER inside the controller MMIO window bound by init().
    unsafe {
        wr(R_SSIENR, 0);
        wr(R_SER, 1 << cs);
        wr(R_SSIENR, 1);
    }
    Ok(())
}

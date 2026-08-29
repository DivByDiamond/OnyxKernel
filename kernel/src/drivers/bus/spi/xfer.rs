//! SPI bus primitives — chip-select control + full-duplex transfer.
use super::*;
use onyx_core::errno::{Errno, KResult};

/// Select a specific chip-select line and hold it (manual CS control).
pub fn select(cs: u8) -> KResult<()> {
    if cs >= MAX_CS {
        return Err(Errno::Inval);
    }
    // SAFETY: wr() accesses R_CSID/R_CSMODE inside the controller MMIO window derived from the probed G_BASE set by spi::init.
    unsafe {
        wr(R_CSID, cs as u32);
        wr(R_CSMODE, CSMODE_HOLD);
    }
    Ok(())
}

/// Release the chip-select line (deasserts CS).
pub fn release() {
    // SAFETY: wr() accesses R_CSMODE inside the controller MMIO window derived from the probed G_BASE.
    unsafe {
        wr(R_CSMODE, CSMODE_AUTO);
    }
}

/// Perform a full-duplex transfer of `len` bytes. `tx` and `rx` must be
/// `len` bytes long; pass zeroes for `tx` on a read-only operation.
pub fn transfer(tx: &[u8], rx: &mut [u8]) -> KResult<()> {
    if tx.len() != rx.len() || tx.is_empty() {
        return Err(Errno::Inval);
    }
    // SAFETY: rd()/wr() access the SPI FIFO registers inside the controller
    // MMIO window derived from the probed G_BASE; rx[i] is bounds-checked by
    // the equal-length contract checked above.
    unsafe {
        for (i, &b) in tx.iter().enumerate() {
            let mut t = TIMEOUT;
            while rd(R_TXDATA) & TX_FULL != 0 {
                t -= 1;
                if t == 0 {
                    return Err(Errno::Io);
                }
            }
            wr(R_TXDATA, b as u32);
            let mut t2 = TIMEOUT;
            loop {
                let v = rd(R_RXDATA);
                if v & RX_EMPTY == 0 {
                    rx[i] = v as u8;
                    break;
                }
                t2 -= 1;
                if t2 == 0 {
                    return Err(Errno::Io);
                }
            }
        }
        Ok(())
    }
}

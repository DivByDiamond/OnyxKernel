//! SPI high-level operations — half-duplex write and read.
use super::*;
use onyx_core::errno::{Errno, KResult};

/// Half-duplex write-only convenience. Sends `tx` and discards the
/// simultaneously-received bytes (always present on SPI full-duplex).
pub fn write(tx: &[u8]) -> KResult<()> {
    if tx.is_empty() {
        return Err(Errno::Inval);
    }
    unsafe {
        for &b in tx {
            let mut t = TIMEOUT;
            while rd(R_TXDATA) & TX_FULL != 0 {
                t -= 1;
                if t == 0 {
                    return Err(Errno::Io);
                }
            }
            wr(R_TXDATA, b as u32);
            // Drain RX FIFO to keep the controller in sync.
            let mut t2 = TIMEOUT;
            loop {
                let v = rd(R_RXDATA);
                if v & RX_EMPTY == 0 {
                    break;
                }
                t2 -= 1;
                if t2 == 0 {
                    return Err(Errno::Io);
                }
            }
        }
    }
    Ok(())
}

/// Half-duplex read. Sends `reg` first then reads `buf.len()` bytes.
pub fn read(reg: u8, buf: &mut [u8]) -> KResult<()> {
    let tx_head = [reg];
    let mut rx_head = [0u8; 1];
    transfer(&tx_head, &mut rx_head)?;
    let zero = [0u8; 64];
    for chunk in buf.chunks_mut(zero.len()) {
        let n = chunk.len();
        transfer(&zero[..n], chunk)?;
    }
    Ok(())
}

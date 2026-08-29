//! I2C bus primitives — start, write/read byte, busy-wait helpers.
use super::*;
use onyx_core::errno::{Errno, KResult};

/// # Safety
/// Caller contract: G_BASE must be a valid probed I2C base (set by i2c::init); no concurrent I2C use.
#[inline]
pub unsafe fn wait_tip() -> KResult<()> {
    // SAFETY: rd() reads R_CMD_STATUS inside the controller MMIO window
    // derived from the probed G_BASE.
    unsafe {
        let mut t = TIMEOUT;
        while t > 0 {
            if rd(R_CMD_STATUS) & S_TIP == 0 {
                return Ok(());
            }
            t -= 1;
        }
        Err(Errno::Io)
    }
}

/// # Safety
/// Caller contract: G_BASE must be a valid probed I2C base; no concurrent I2C use.
#[inline]
pub unsafe fn wait_not_busy() -> KResult<()> {
    // SAFETY: rd() reads R_CMD_STATUS inside the controller MMIO window derived from the probed G_BASE.
    unsafe {
        let mut t = TIMEOUT;
        while t > 0 {
            if rd(R_CMD_STATUS) & S_BUSY == 0 {
                return Ok(());
            }
            t -= 1;
        }
        Err(Errno::Busy)
    }
}

/// Issue START + address + R/W bit and wait for ACK from the slave.
/// # Safety
/// Caller contract: G_BASE must be a valid probed I2C base (set by i2c::init); no concurrent I2C bus use during the transaction.
pub unsafe fn start(addr: u8, read: bool) -> KResult<()> {
    // SAFETY: wr()/rd() access R_TXRX/R_CMD_STATUS within the controller MMIO window derived from the probed G_BASE.
    unsafe {
        let byte = (addr << 1) | (if read { 1 } else { 0 });
        wr(R_TXRX, byte as u32);
        wr(R_CMD_STATUS, STA | WR);
        wait_tip()?;
        if rd(R_CMD_STATUS) & S_RXACK != 0 {
            return Err(Errno::NoEnt);
        }
        Ok(())
    }
}

/// Write a single byte with optional STOP. Returns Err on NACK.
/// # Safety
/// Caller contract: G_BASE must be a valid probed I2C base; no concurrent I2C bus use.
pub unsafe fn write_byte(byte: u8, stop: bool) -> KResult<()> {
    // SAFETY: wr()/rd() access R_TXRX/R_CMD_STATUS within the controller MMIO window derived from the probed G_BASE.
    unsafe {
        wr(R_TXRX, byte as u32);
        let cmd = WR | if stop { STO } else { 0 };
        wr(R_CMD_STATUS, cmd);
        wait_tip()?;
        if rd(R_CMD_STATUS) & S_RXACK != 0 {
            return Err(Errno::Io);
        }
        Ok(())
    }
}

/// Read a single byte. `ack=false` on the last byte to NACK the slave.
/// # Safety
/// Caller contract: G_BASE must be a valid probed I2C base; no concurrent I2C bus use.
pub unsafe fn read_byte(ack: bool, stop: bool) -> KResult<u8> {
    // SAFETY: wr()/rd() access R_CMD_STATUS/R_TXRX within the controller MMIO window derived from the probed G_BASE.
    unsafe {
        let cmd = if ack {
            RD | ACK | if stop { STO } else { 0 }
        } else {
            RD | if stop { STO } else { 0 }
        };
        wr(R_CMD_STATUS, cmd);
        wait_tip()?;
        Ok(rd(R_TXRX) as u8)
    }
}

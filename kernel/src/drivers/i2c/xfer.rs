//! I2C bus primitives — start, write/read byte, busy-wait helpers.
use super::*;
use onyx_core::errno::{Errno, KResult};

#[inline]
pub unsafe fn wait_tip() -> KResult<()> {
    let mut t = TIMEOUT;
    while t > 0 {
        if rd(R_CMD_STATUS) & S_TIP == 0 {
            return Ok(());
        }
        t -= 1;
    }
    Err(Errno::Io)
}

#[inline]
pub unsafe fn wait_not_busy() -> KResult<()> {
    let mut t = TIMEOUT;
    while t > 0 {
        if rd(R_CMD_STATUS) & S_BUSY == 0 {
            return Ok(());
        }
        t -= 1;
    }
    Err(Errno::Busy)
}

/// Issue START + address + R/W bit and wait for ACK from the slave.
pub unsafe fn start(addr: u8, read: bool) -> KResult<()> {
    let byte = (addr << 1) | (if read { 1 } else { 0 });
    wr(R_TXRX, byte as u32);
    wr(R_CMD_STATUS, STA | WR);
    wait_tip()?;
    if rd(R_CMD_STATUS) & S_RXACK != 0 {
        return Err(Errno::NoEnt);
    }
    Ok(())
}

/// Write a single byte with optional STOP. Returns Err on NACK.
pub unsafe fn write_byte(byte: u8, stop: bool) -> KResult<()> {
    wr(R_TXRX, byte as u32);
    let cmd = WR | if stop { STO } else { 0 };
    wr(R_CMD_STATUS, cmd);
    wait_tip()?;
    if rd(R_CMD_STATUS) & S_RXACK != 0 {
        return Err(Errno::Io);
    }
    Ok(())
}

/// Read a single byte. `ack=false` on the last byte to NACK the slave.
pub unsafe fn read_byte(ack: bool, stop: bool) -> KResult<u8> {
    let cmd = if ack {
        RD | ACK | if stop { STO } else { 0 }
    } else {
        RD | if stop { STO } else { 0 }
    };
    wr(R_CMD_STATUS, cmd);
    wait_tip()?;
    Ok(rd(R_TXRX) as u8)
}

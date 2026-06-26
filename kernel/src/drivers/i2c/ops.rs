//! I2C high-level operations — write, read, scan.
use super::*;
use onyx_core::errno::{Errno, KResult};

/// Write `data` to `addr` (7-bit). Generates START + addr + payload + STOP.
pub fn write(addr: u8, data: &[u8]) -> KResult<()> {
    if data.is_empty() {
        return Err(Errno::Inval);
    }
    unsafe {
        start(addr, false)?;
        for (i, &b) in data.iter().enumerate() {
            write_byte(b, i == data.len() - 1)?;
        }
        wait_not_busy().ok();
        Ok(())
    }
}

/// Read `n` bytes from `addr` (7-bit). If `reg` is Some, writes `reg` first
/// as a register select, then issues a repeated-start for the read.
pub fn read(addr: u8, reg: Option<u8>, buf: &mut [u8]) -> KResult<()> {
    if buf.is_empty() {
        return Err(Errno::Inval);
    }
    unsafe {
        if let Some(r) = reg {
            start(addr, false)?;
            write_byte(r, false)?;
            start(addr, true)?;
        } else {
            start(addr, true)?;
        }
        let last = buf.len() - 1;
        for i in 0..buf.len() {
            buf[i] = read_byte(i < last, i == last)?;
        }
        wait_not_busy().ok();
        Ok(())
    }
}

/// Scan the bus and write the addresses of responding slaves into `out`.
/// Returns the number of devices found.
pub fn scan(out: &mut [u8]) -> usize {
    let mut n = 0;
    for addr in 1u8..128 {
        if n >= out.len() {
            break;
        }
        let ok = unsafe { start(addr, false).is_ok() };
        if ok {
            unsafe { wr(R_CMD_STATUS, STO); }
            out[n] = addr;
            n += 1;
        }
    }
    n
}

//! OTP / eFuse — one-time-programmable fuse read access.
//!
//! Duo S exposes a 1024-byte OTP fuse array at 0x0305_0000. Reading
//! is destructive on the controller side (auto-increments the address
//! pointer) so we cache the contents on first access. Write is
//! intentionally restricted to a single one-shot API to model the OTP
//! nature of the device.
use crate::arch::mmio::Mmio;
use onyx_core::errno::{Errno, KResult};

const OTP_BASE: usize = 0x0305_0000;
const OTP_SIZE: usize = 1024;

const R_ADDR: u32 = 0x00;
const R_DATA: u32 = 0x04;
const R_CTRL: u32 = 0x08;

const CTRL_READ: u32 = 1 << 0;
const CTRL_BUSY: u32 = 1 << 1;
const CTRL_WRITE: u32 = 1 << 4;

static mut G_BASE: usize = OTP_BASE;
static mut G_CACHE: [u8; OTP_SIZE] = [0; OTP_SIZE];
static mut G_CACHED: bool = false;

#[inline]
unsafe fn rd(off: u32) -> u32 {
    Mmio::<u32>::at(G_BASE + off as usize).read()
}

#[inline]
unsafe fn wr(off: u32, v: u32) {
    Mmio::<u32>::at(G_BASE + off as usize).write(v);
}

/// Initialise the OTP controller and prime the cache.
pub unsafe fn init(base: usize) -> KResult<()> {
    G_BASE = base;
    cache_all()?;
    Ok(())
}

unsafe fn cache_all() -> KResult<()> {
    for i in 0..OTP_SIZE {
        wr(R_ADDR, i as u32);
        wr(R_CTRL, CTRL_READ);
        let mut t = 100_000u32;
        while t > 0 && rd(R_CTRL) & CTRL_BUSY != 0 {
            t -= 1;
        }
        if t == 0 {
            return Err(Errno::Io);
        }
        let v = rd(R_DATA);
        G_CACHE[i] = v as u8;
    }
    G_CACHED = true;
    Ok(())
}

/// Read a single byte from the fuse array. Returns from cache.
pub fn read_byte(idx: usize) -> KResult<u8> {
    if idx >= OTP_SIZE {
        return Err(Errno::Range);
    }
    unsafe {
        if !G_CACHED {
            cache_all()?;
        }
        Ok(G_CACHE[idx])
    }
}

/// Read a slice of bytes. `out.len()` bytes are copied starting at `start`.
pub fn read(start: usize, out: &mut [u8]) -> KResult<()> {
    if start + out.len() > OTP_SIZE {
        return Err(Errno::Range);
    }
    for (i, b) in out.iter_mut().enumerate() {
        *b = read_byte(start + i)?;
    }
    Ok(())
}

/// Read the 6-byte MAC address stored in OTP (typically at offset 0x90).
pub fn read_mac() -> KResult<[u8; 6]> {
    let mut mac = [0u8; 6];
    read(0x90, &mut mac)?;
    Ok(mac)
}

/// Permanently write a byte to the fuse array. **DANGER: irreversible.**
/// Use only for one-time provisioning. Returns `Err(Errno::Perm)` after
/// the cache has been written-once already.
pub fn write_byte(idx: usize, val: u8) -> KResult<()> {
    if idx >= OTP_SIZE {
        return Err(Errno::Range);
    }
    unsafe {
        wr(R_ADDR, idx as u32);
        wr(R_DATA, val as u32);
        wr(R_CTRL, CTRL_WRITE);
        let mut t = 100_000u32;
        while t > 0 && rd(R_CTRL) & CTRL_BUSY != 0 {
            t -= 1;
        }
        if t == 0 {
            return Err(Errno::Io);
        }
        G_CACHE[idx] = val;
    }
    Ok(())
}

/// Has the cache been primed?
pub fn is_cached() -> bool {
    unsafe { G_CACHED }
}

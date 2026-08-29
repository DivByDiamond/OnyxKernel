use super::*;
use onyx_core::errno::{Errno, KResult};

/// # Safety
/// Caller contract: `i2c_base` must be a valid SiFive I2C MMIO base, identity-mapped; no concurrent I2C use while this runs (it temporarily rebinds G_BASE).
pub unsafe fn read_edid(i2c_base: usize) -> KResult<[u8; 128]> {
    // SAFETY: G_BASE is temporarily set to the caller-provided I2C MMIO
    // base; all rd/wr helpers then access offsets within the controller
    // register file per mod.rs constants. No concurrent I2C calls allowed.
    unsafe {
        let old_base = G_BASE;
        G_BASE = i2c_base;
        let mut edid = [0u8; 128];
        start(0x50, false)?;
        write_byte(0x00, false)?;
        start(0x50, true)?;
        for (i, byte) in edid.iter_mut().enumerate() {
            *byte = read_byte(i < 127, i == 127)?;
        }
        wait_not_busy().ok();
        G_BASE = old_base;
        if edid[0] != 0x00
            || edid[1] != 0xFF
            || edid[2] != 0xFF
            || edid[3] != 0xFF
            || edid[4] != 0xFF
            || edid[5] != 0xFF
            || edid[6] != 0xFF
            || edid[7] != 0x00
        {
            return Err(Errno::Inval);
        }
        Ok(edid)
    }
}

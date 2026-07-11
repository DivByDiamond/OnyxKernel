use super::*;
use onyx_core::errno::{Errno, KResult};

pub unsafe fn read_edid(i2c_base: usize) -> KResult<[u8; 128]> {
    let old_base = G_BASE;
    G_BASE = i2c_base;
    let mut edid = [0u8; 128];
    start(0x50, false)?;
    write_byte(0x00, false)?;
    start(0x50, true)?;
    for i in 0..128 {
        edid[i] = read_byte(i < 127, i == 127)?;
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

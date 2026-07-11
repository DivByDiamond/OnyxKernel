use super::{FdToken, PERM_SEEK, fd_check_perm, fd_get, fd_update_pos};
use onyx_core::errno::{Errno, KResult};

pub unsafe fn lseek(token: FdToken, off: i64, whence: u32) -> KResult<u32> {
    let idx = fd_check_perm(token, PERM_SEEK)?;
    let fd = fd_get(idx);
    let new_pos: i64 = match whence {
        0 => off,
        1 => fd.pos as i64 + off,
        2 => fd.size as i64 + off,
        _ => return Err(Errno::Inval),
    };
    if new_pos < 0 || new_pos > fd.size as i64 {
        return Err(Errno::Range);
    }
    fd_update_pos(idx, new_pos as u32);
    Ok(new_pos as u32)
}

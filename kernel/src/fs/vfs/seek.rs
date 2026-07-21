use super::{fd_check_perm, fd_get, fd_update_pos, FdToken, PERM_SEEK};
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
    // Bug (fs SERIOUS #2): allow seeking past EOF. POSIX permits lseek
    // past end of file (subsequent reads return 0 / short read); the
    // previous code rejected any new_pos > fd.size with ERANGE, which
    // broke legitimate patterns like lseek(fd, 0, SEEK_END) + 1 to
    // reserve space, and broke libc's fseek() to end+1. We still reject
    // negative positions. For regular files this creates a sparse
    // region that reads as zeros until a write fills it in.
    if new_pos < 0 {
        return Err(Errno::Range);
    }
    // Cap at u32::MAX (VFS positions are u32). Slightly past EOF is OK.
    if new_pos > u32::MAX as i64 {
        return Err(Errno::Range);
    }
    fd_update_pos(idx, new_pos as u32);
    Ok(new_pos as u32)
}

use super::{fd_check, fd_check_perm, fd_get, fd_update_pos, FdToken, Fs, PERM_READ, PERM_WRITE};
use crate::fs::{fat32, ipcfs, onyxfs, procfs};
use onyx_core::errno::{Errno, KResult};

pub unsafe fn read(token: FdToken, buf: *mut u8, len: u32) -> KResult<u32> {
    let idx = fd_check_perm(token, PERM_READ)?;
    let fd = fd_get(idx);
    let avail = fd.size.saturating_sub(fd.pos);
    let to_read = len.min(avail);
    if to_read == 0 {
        return Ok(0);
    }
    let read_n = match fd.fs {
        Fs::Onyx => onyxfs::read(fd.ino, buf, fd.pos, to_read)?,
        Fs::Fat32 => fat32::read(fd.ino, buf, fd.pos, to_read)?,
        Fs::Proc => procfs::read(fd.ino, buf, fd.pos, to_read)?,
        Fs::Ipc => ipcfs::read(fd.ino, buf, fd.pos, to_read)?,
        Fs::None => return Err(Errno::Inval),
    };
    fd_update_pos(idx, fd.pos + read_n);
    Ok(read_n)
}

pub unsafe fn write(token: FdToken, buf: *const u8, len: u32) -> KResult<u32> {
    let idx = fd_check_perm(token, PERM_WRITE)?;
    let fd = fd_get(idx);
    let written = match fd.fs {
        Fs::Onyx => onyxfs::write(fd.ino, buf, fd.pos, len)?,
        Fs::Proc => return Err(Errno::Perm),
        Fs::Ipc => ipcfs::write(fd.ino, buf, fd.pos, len)?,
        _ => return Err(Errno::NoSys),
    };
    let new_pos = fd.pos + written;
    fd_update_pos(idx, new_pos);
    if new_pos > fd.size {
        if crate::fs::vfs::ops::is_kernel_boot() {
            let p = &raw mut crate::fs::vfs::ops::G_KERNEL_FDS;
            (*p)[idx].size = new_pos;
        } else {
            let p = crate::proc::current();
            p.fds[idx].size = new_pos;
        }
    }
    Ok(written)
}

pub unsafe fn stat(token: FdToken, size_out: &mut u32) -> KResult<()> {
    let idx = fd_check(token)?;
    let fd = fd_get(idx);
    *size_out = fd.size;
    Ok(())
}

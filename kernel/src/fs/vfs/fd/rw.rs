use crate::fs::vfs::{
    FdToken, Fs, PERM_READ, PERM_WRITE, fd_check, fd_check_perm, fd_get, fd_update_pos,
};
use crate::fs::{devfs, fat32, ipcfs, onyxfs, procfs};
use onyx_core::errno::{Errno, KResult};

/// # Safety
///
/// Caller contract: for user callers, `buf` must be a validated user range
/// of `len` bytes (syscall layer checked user_ptr_ok/check_user_range and
/// translated it); kernel callers pass a valid kernel buffer. Token must be
/// a live fd token.
pub unsafe fn read(token: FdToken, buf: *mut u8, len: u32) -> KResult<u32> {
    // SAFETY: fd_check_perm validates idx and epoch; the backend read fns
    // receive a buffer the syscall layer already validated/translated for
    // `len` bytes (user_ptr_ok), and the position math is saturating.
    unsafe {
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
            Fs::Devfs => devfs::read(fd.ino, buf, fd.pos, to_read)?,
            Fs::None => return Err(Errno::Inval),
        };
        fd_update_pos(idx, fd.pos + read_n);
        Ok(read_n)
    }
}

/// # Safety
///
/// Caller contract: for user callers, `buf` must be a validated, readable
/// user range of `len` bytes (checked and translated by the syscall layer);
/// kernel callers pass a valid kernel buffer. Token must be a live fd token.
pub unsafe fn write(token: FdToken, buf: *const u8, len: u32) -> KResult<u32> {
    // SAFETY: fd_check_perm validates idx and epoch; backends get a buffer
    // the syscall layer validated for `len` bytes. The direct G_KERNEL_FDS /
    // p.fds size write below targets the current context's fd slot with a
    // checked idx, same contract as fd_update_pos.
    unsafe {
        let idx = fd_check_perm(token, PERM_WRITE)?;
        let fd = fd_get(idx);
        let written = match fd.fs {
            Fs::Onyx => onyxfs::write(fd.ino, buf, fd.pos, len)?,
            Fs::Proc => return Err(Errno::Perm),
            Fs::Ipc => ipcfs::write(fd.ino, buf, fd.pos, len)?,
            _ => return Err(Errno::NoSys),
        };
        // Bug (fs SERIOUS #1): use checked_add for the new position. The
        // previous code did `fd.pos + written` which silently wraps on
        // overflow — a 4 GB file with pos near u32::MAX would wrap to a
        // tiny position and corrupt subsequent reads/writes.
        let new_pos = fd.pos.checked_add(written).ok_or(Errno::NoMem)?;
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
}

/// # Safety
///
/// Caller contract: token must be a live fd token; size_out is a kernel-side
/// &mut so always valid.
pub unsafe fn stat(token: FdToken, size_out: &mut u32) -> KResult<()> {
    // SAFETY: fd_check validates idx and epoch; fd_get returns a plain copy,
    // and size_out is a kernel-side mutable reference.
    unsafe {
        let idx = fd_check(token)?;
        let fd = fd_get(idx);
        *size_out = fd.size;
        Ok(())
    }
}

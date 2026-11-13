use crate::fs::vfs::{
    FdToken, Fs, PERM_READ, PERM_WRITE, fd_check, fd_check_perm, fd_get, fd_update_pos, with_mount,
};
use crate::fs::{devfs, fat32, ipcfs, onyxfs, procfs};
use crate::syscall::abi::O_APPEND;
use onyx_core::errno::{Errno, KResult};

/// First write offset of a descriptor: with `O_APPEND` the current end of the
/// file, otherwise the descriptor's own position.
fn write_start(flags: u32, pos: u32, end_of_file: u32) -> u32 {
    if flags & O_APPEND != 0 {
        end_of_file
    } else {
        pos
    }
}

/// Where a write through `fd` starts. An appending descriptor asks the inode
/// for the real end of file instead of trusting its cached position: a spawned
/// child that inherited the same bound standard stream may have extended the
/// file in the meantime.
///
/// # Safety
///
/// `fd` is a live descriptor of the calling context.
unsafe fn write_offset(fd: &crate::fs::vfs::VfsFd) -> KResult<u32> {
    // SAFETY: the stat reads only the inode behind `fd.ino` inside the fd's
    // own mount context, as the open path does.
    unsafe {
        if fd.flags & O_APPEND == 0 || !matches!(fd.fs, Fs::Onyx) {
            return Ok(fd.pos);
        }
        let mut st = onyxfs::OnyfsStat::default();
        with_mount(fd.mnt as usize, || onyxfs::stat(fd.ino, &mut st))?;
        Ok(write_start(
            fd.flags,
            fd.pos,
            u32::try_from(st.size).unwrap_or(u32::MAX),
        ))
    }
}

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
    // with_mount activates the fd's mount context (root for pseudo fds).
    unsafe {
        let idx = fd_check_perm(token, PERM_READ)?;
        let fd = fd_get(idx);
        let avail = fd.size.saturating_sub(fd.pos);
        let to_read = len.min(avail);
        if to_read == 0 {
            return Ok(0);
        }
        let mnt = fd.mnt as usize;
        let read_n = match fd.fs {
            Fs::Onyx => with_mount(mnt, || onyxfs::read(fd.ino, buf, fd.pos, to_read))?,
            Fs::Fat32 => with_mount(mnt, || fat32::read(fd.ino, buf, fd.pos, to_read))?,
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
    // checked idx, same contract as fd_update_pos. with_mount holds FS_LOCK
    // and runs the write in the fd's mount context.
    unsafe {
        let idx = fd_check_perm(token, PERM_WRITE)?;
        let mut fd = fd_get(idx);
        fd.pos = write_offset(&fd)?;
        let mnt = fd.mnt as usize;
        let written = match fd.fs {
            Fs::Onyx => with_mount(mnt, || onyxfs::write(fd.ino, buf, fd.pos, len))?,
            Fs::Fat32 => {
                // A dirent without a first cluster cannot be located from the
                // fd alone (it carries parent + cluster, not the name), so
                // foreign zero-length files stay read-only here. Files created
                // through this VFS always get a cluster in fat32::create.
                if fd.ino == 0 {
                    return Err(Errno::NoSys);
                }
                let written = with_mount(mnt, || fat32::write(fd.ino, buf, fd.pos, len))?;
                // Keep the on-disk dirent size in step with the file's growth;
                // fd.size itself is updated by the common path below.
                let grown_to = fd.pos.checked_add(written).ok_or(Errno::NoMem)?;
                if grown_to > fd.size {
                    with_mount(mnt, || fat32::set_size(fd.dir, fd.ino, grown_to))?;
                }
                written
            }
            Fs::Proc => return Err(Errno::Perm),
            Fs::Ipc => ipcfs::write(fd.ino, buf, fd.pos, len)?,
            Fs::Devfs => devfs::write(fd.ino, buf, fd.pos, len)?,
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

#[cfg(test)]
mod tests;

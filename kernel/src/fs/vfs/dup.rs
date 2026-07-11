use super::{
    FdToken, Fs, PERM_READ, PERM_WRITE, VFS_MAX_FDS, alloc_fd, fd_check, fd_get, fd_set, fd_token,
};
use onyx_core::errno::{Errno, KResult};

pub unsafe fn dup(token: FdToken) -> KResult<FdToken> {
    let idx = fd_check(token)?;
    let fd = fd_get(idx);
    let new_idx = alloc_fd(fd.perms)?;
    fd_set(new_idx, fd.ino, fd.size, fd.fs, fd.pos);
    let new_fd = fd_get(new_idx);
    Ok(fd_token(new_idx, new_fd.epoch))
}

pub unsafe fn dup2(old_token: FdToken, new_fd: u64) -> KResult<FdToken> {
    let idx = fd_check(old_token)?;
    let fd = fd_get(idx);
    let new_idx = new_fd as usize;
    if new_idx >= VFS_MAX_FDS {
        return Err(Errno::BadFd);
    }
    if crate::fs::vfs::ops::is_kernel_boot() {
        let p = &raw mut crate::fs::vfs::ops::G_KERNEL_FDS;
        (*p)[new_idx].used = false;
        (*p)[new_idx].used = true;
        (*p)[new_idx].perms = fd.perms;
        (*p)[new_idx].epoch = (*p)[new_idx].epoch.wrapping_add(1);
        if (*p)[new_idx].epoch == 0 {
            (*p)[new_idx].epoch = 1;
        }
        (*p)[new_idx].ino = fd.ino;
        (*p)[new_idx].size = fd.size;
        (*p)[new_idx].fs = fd.fs;
        (*p)[new_idx].pos = fd.pos;
    } else {
        let p = crate::proc::current();
        p.fds[new_idx].used = false;
        p.fds[new_idx].used = true;
        p.fds[new_idx].perms = fd.perms;
        p.fds[new_idx].epoch = p.fds[new_idx].epoch.wrapping_add(1);
        if p.fds[new_idx].epoch == 0 {
            p.fds[new_idx].epoch = 1;
        }
        p.fds[new_idx].ino = fd.ino;
        p.fds[new_idx].size = fd.size;
        p.fds[new_idx].fs = fd.fs;
        p.fds[new_idx].pos = fd.pos;
    }
    let new_fd_entry = fd_get(new_idx);
    Ok(fd_token(new_idx, new_fd_entry.epoch))
}

pub unsafe fn create_pipe() -> KResult<(FdToken, FdToken)> {
    let r_idx = alloc_fd(PERM_READ)?;
    let w_idx = alloc_fd(PERM_WRITE)?;
    let pipe_ino = !0u32;
    if crate::fs::vfs::ops::is_kernel_boot() {
        let p = &raw mut crate::fs::vfs::ops::G_KERNEL_FDS;
        (*p)[r_idx].ino = pipe_ino;
        (*p)[r_idx].size = 0;
        (*p)[r_idx].fs = Fs::Ipc;
        (*p)[r_idx].pos = 0;
        (*p)[w_idx].ino = pipe_ino;
        (*p)[w_idx].size = 0;
        (*p)[w_idx].fs = Fs::Ipc;
        (*p)[w_idx].pos = 0;
    } else {
        let p = crate::proc::current();
        p.fds[r_idx].ino = pipe_ino;
        p.fds[r_idx].size = 0;
        p.fds[r_idx].fs = Fs::Ipc;
        p.fds[r_idx].pos = 0;
        p.fds[w_idx].ino = pipe_ino;
        p.fds[w_idx].size = 0;
        p.fds[w_idx].fs = Fs::Ipc;
        p.fds[w_idx].pos = 0;
    }
    let r_fd = fd_get(r_idx);
    let w_fd = fd_get(w_idx);
    Ok((fd_token(r_idx, r_fd.epoch), fd_token(w_idx, w_fd.epoch)))
}

use onyx_core::errno::{Errno, KResult};

use super::vnode::{fd_token_epoch, fd_token_idx, Fs, VfsFd, VFS_MAX_FDS};

pub(crate) unsafe fn is_kernel_boot() -> bool {
    crate::proc::current_pid() == 0
}

pub(crate) static mut G_KERNEL_FDS: [VfsFd; VFS_MAX_FDS] = [VfsFd {
    ino: 0,
    size: 0,
    pos: 0,
    fs: Fs::None,
    used: false,
    perms: 0,
    epoch: 0,
}; VFS_MAX_FDS];

pub unsafe fn init() {}

pub(crate) unsafe fn alloc_fd(perms: u32) -> KResult<usize> {
    if is_kernel_boot() {
        let p = &raw mut G_KERNEL_FDS;
        for i in 0..VFS_MAX_FDS {
            if !(*p)[i].used {
                (*p)[i].used = true;
                (*p)[i].perms = perms;
                (*p)[i].epoch = (*p)[i].epoch.wrapping_add(1);
                if (*p)[i].epoch == 0 {
                    (*p)[i].epoch = 1;
                }
                return Ok(i);
            }
        }
        return Err(Errno::NoMem);
    }
    let p = crate::proc::current();
    // Skip fds 0-2 (stdin/stdout/stderr) which are handled by UART directly
    // for user-space processes (all rings). Kernel boot uses ring 0 but there
    // is no UART redirection for kernel fds, so we skip unconditionally here
    // and kernel-boot fds come from G_KERNEL_FDS above.
    for i in 3..VFS_MAX_FDS {
        if !p.fds[i].used {
            p.fds[i].used = true;
            p.fds[i].perms = perms;
            p.fds[i].epoch = p.fds[i].epoch.wrapping_add(1);
            if p.fds[i].epoch == 0 {
                p.fds[i].epoch = 1;
            }
            return Ok(i);
        }
    }
    Err(Errno::NoMem)
}

pub(crate) unsafe fn fd_check(token: super::vnode::FdToken) -> KResult<usize> {
    let idx = fd_token_idx(token);
    if idx >= VFS_MAX_FDS {
        return Err(Errno::BadFd);
    }
    let fd = fd_get(idx);
    if !fd.used || fd.epoch != fd_token_epoch(token) {
        return Err(Errno::BadFd);
    }
    Ok(idx)
}

pub(crate) unsafe fn fd_check_perm(token: super::vnode::FdToken, perm: u32) -> KResult<usize> {
    let idx = fd_check(token)?;
    let fd = fd_get(idx);
    if fd.perms & perm == 0 {
        return Err(Errno::Perm);
    }
    Ok(idx)
}

pub(crate) unsafe fn fd_get(idx: usize) -> VfsFd {
    if is_kernel_boot() {
        let p = &raw const G_KERNEL_FDS;
        (*p)[idx]
    } else {
        let p = crate::proc::current();
        p.fds[idx]
    }
}

pub(crate) unsafe fn fd_set(idx: usize, ino: u32, size: u32, fs: Fs, pos: u32) {
    if is_kernel_boot() {
        let p = &raw mut G_KERNEL_FDS;
        (*p)[idx].ino = ino;
        (*p)[idx].size = size;
        (*p)[idx].fs = fs;
        (*p)[idx].pos = pos;
    } else {
        let p = crate::proc::current();
        p.fds[idx].ino = ino;
        p.fds[idx].size = size;
        p.fds[idx].fs = fs;
        p.fds[idx].pos = pos;
    }
}

pub(crate) unsafe fn fd_update_pos(idx: usize, pos: u32) {
    if is_kernel_boot() {
        let p = &raw mut G_KERNEL_FDS;
        (*p)[idx].pos = pos;
    } else {
        let p = crate::proc::current();
        p.fds[idx].pos = pos;
    }
}

pub(crate) unsafe fn fd_clear(idx: usize) {
    if is_kernel_boot() {
        let p = &raw mut G_KERNEL_FDS;
        (*p)[idx].used = false;
    } else {
        let p = crate::proc::current();
        p.fds[idx].used = false;
    }
}

pub unsafe fn rename(old_path: &[u8], new_path: &[u8]) -> KResult<()> {
    crate::fs::onyxfs::rename(old_path, new_path)
}

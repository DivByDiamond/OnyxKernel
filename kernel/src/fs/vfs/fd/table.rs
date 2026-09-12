//! Direct fd-table accessors (boot/proc table slot reads and writes).
//!
//! Split out of `ops` so the allocation/permission logic and the raw table
//! plumbing can evolve separately (file-size rule, see `info.md`).

use crate::fs::vfs::vnode::{Fs, VfsFd};

use super::ops::{G_KERNEL_FDS, is_kernel_boot};

/// # Safety
///
/// Caller contract: idx < VFS_MAX_FDS, validated by fd_check/fd_check_perm
/// at the call site; runs in the fd-owning process's syscall context.
/// Returns a snapshot copy, so no aliasing survives the call.
pub(crate) unsafe fn fd_get(idx: usize) -> VfsFd {
    // SAFETY: idx is caller-validated (< VFS_MAX_FDS) via fd_check upstream;
    // plain copy out of the fd table of the current context (see # Safety).
    unsafe {
        if is_kernel_boot() {
            let p = &raw const G_KERNEL_FDS;
            (*p)[idx]
        } else {
            let p = crate::proc::current();
            p.fds[idx]
        }
    }
}

/// Fill a slot freshly claimed by `alloc_fd`, including its mount slot
/// (`mnt`: a `G_MOUNTS` index, or `MNT_ROOT as u8` for the root fs).
///
/// # Safety
///
/// Caller contract: idx < VFS_MAX_FDS and is the slot allocated for this
/// open (allocated by alloc_fd); runs in the fd-owning context.
pub(crate) unsafe fn fd_set(idx: usize, ino: u32, size: u32, fs: Fs, pos: u32, mnt: u8) {
    // SAFETY: idx is the slot just claimed by alloc_fd in this context;
    // writing it cannot race with any other user of that slot.
    unsafe {
        if is_kernel_boot() {
            let p = &raw mut G_KERNEL_FDS;
            (*p)[idx].ino = ino;
            (*p)[idx].size = size;
            (*p)[idx].fs = fs;
            (*p)[idx].pos = pos;
            (*p)[idx].mnt = mnt;
        } else {
            let p = crate::proc::current();
            p.fds[idx].ino = ino;
            p.fds[idx].size = size;
            p.fds[idx].fs = fs;
            p.fds[idx].pos = pos;
            p.fds[idx].mnt = mnt;
        }
    }
}

/// # Safety
///
/// Caller contract: idx < VFS_MAX_FDS, validated by fd_check at the call
/// site; runs in the fd-owning process's syscall context.
pub(crate) unsafe fn fd_clear(idx: usize) {
    // SAFETY: idx is caller-validated via fd_check upstream; marks the slot
    // unused in the owning context's table only.
    unsafe {
        if is_kernel_boot() {
            let p = &raw mut G_KERNEL_FDS;
            (*p)[idx].used = false;
        } else {
            let p = crate::proc::current();
            p.fds[idx].used = false;
        }
    }
}

/// # Safety
///
/// Caller contract: idx < VFS_MAX_FDS (callers obtain it from fd_check /
/// fd_check_perm); must run in the fd-owning process's syscall context or
/// kernel boot. Writes only the pos field of the owning context's slot.
pub(crate) unsafe fn fd_update_pos(idx: usize, pos: u32) {
    // SAFETY: idx is caller-validated via fd_check upstream.
    unsafe {
        if is_kernel_boot() {
            let p = &raw mut G_KERNEL_FDS;
            (*p)[idx].pos = pos;
        } else {
            let p = crate::proc::current();
            p.fds[idx].pos = pos;
        }
    }
}

/// # Safety
///
/// Caller contract: idx < VFS_MAX_FDS, obtained from fd_check() at the call
/// site (e.g. sys_fcntl F_SETFD); runs in the fd-owning process's syscall
/// context.
pub(crate) unsafe fn fd_set_cloexec(idx: usize, cloexec: bool) {
    // SAFETY: idx is pre-validated (< VFS_MAX_FDS) by the fd_check call at
    // the call site; the table written is this hart's current process's.
    unsafe {
        if is_kernel_boot() {
            let p = &raw mut G_KERNEL_FDS;
            (*p)[idx].cloexec = cloexec;
        } else {
            let p = crate::proc::current();
            p.fds[idx].cloexec = cloexec;
        }
    }
}

/// # Safety
///
/// Caller contract: idx < VFS_MAX_FDS from fd_check() (F_SETFL/sys_open);
/// runs in the fd-owning process's syscall context.
pub(crate) unsafe fn fd_set_flags(idx: usize, flags: u32) {
    // SAFETY: idx is pre-validated (< VFS_MAX_FDS) by the fd_check call at
    // the call site; the table written is this hart's current process's.
    unsafe {
        if is_kernel_boot() {
            let p = &raw mut G_KERNEL_FDS;
            (*p)[idx].flags = flags;
        } else {
            let p = crate::proc::current();
            p.fds[idx].flags = flags;
        }
    }
}

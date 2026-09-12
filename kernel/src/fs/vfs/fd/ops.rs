//! FD allocation, token validation, and cross-mount rename dispatch.
//!
//! Raw table accessors (`fd_get`/`fd_set`/...) live in the sibling `table`
//! module; both are re-exported through `vfs`.

use onyx_core::errno::{Errno, KResult};

use super::table::fd_get;
use crate::fs::vfs::mount::{rel_to_abs, resolve_path, with_mount};
use crate::fs::vfs::vnode::{Fs, MAX_MOUNTS, VFS_MAX_FDS, VfsFd, fd_token_epoch, fd_token_idx};

/// # Safety
///
/// Reads this hart's current-process slot; safe for any kernel context.
///
/// Kernel FD table is used when the current hart has no process context.
/// A user process may only allocate FDs from its own syscall context, so
/// a missing current slot is either boot-time kernel work or a service task
/// running without a process; both must use the kernel table.
pub(crate) unsafe fn is_kernel_boot() -> bool {
    crate::proc::current_opt().is_none()
}

pub(crate) static mut G_KERNEL_FDS: [VfsFd; VFS_MAX_FDS] = [VfsFd {
    ino: 0,
    size: 0,
    pos: 0,
    fs: Fs::None,
    used: false,
    perms: 0,
    epoch: 0,
    cloexec: false,
    flags: 0,
    mnt: crate::fs::vfs::mount::MNT_ROOT as u8,
}; VFS_MAX_FDS];

/// # Safety
///
/// No-op initializer; safe in every context. Kept unsafe for symmetry with
/// the other fd-table entry points.
pub unsafe fn init() {}

/// # Safety
///
/// Caller contract: must run in the fd-owning context - either kernel-boot
/// init (pid 0, sole user of G_KERNEL_FDS on the boot hart) or a syscall of
/// the process current on this hart. Kernel code runs with SIE=0 (no
/// same-hart preemption) and a process executes syscalls on one hart at a
/// time, so the scan-and-claim is not interleaved (see crate::sync).
pub(crate) unsafe fn alloc_fd(perms: u32) -> KResult<usize> {
    // SAFETY: kernel-boot path touches G_KERNEL_FDS, which only pid 0 uses
    // during boot; otherwise we mutate p.fds of the process current on this
    // hart, which only its own syscall context can access (see # Safety).
    unsafe {
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
            // Bug (fs MINOR #3): return BadFd (EMFILE) instead of NoMem when
            // the FD table is full. POSIX distinguishes EMFILE (per-process FD
            // limit reached) from ENOMEM (out of memory). The previous code
            // returned NoMem which made libc report 'Out of memory' instead
            // of 'Too many open files'.
            return Err(Errno::BadFd);
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
        // Bug (fs MINOR #3): same as above — EMFILE, not ENOMEM.
        Err(Errno::BadFd)
    }
}

/// # Safety
///
/// Caller contract: token must come from fd_token() of a live fd; this
/// re-validates idx (< VFS_MAX_FDS) and the epoch itself (low 12 bits,
/// see vnode::fd_token for compact 32-bit encoding).
pub(crate) unsafe fn fd_check(token: crate::fs::vfs::vnode::FdToken) -> KResult<usize> {
    // SAFETY: bounds-checks idx (< VFS_MAX_FDS) and the epoch before any
    // fd-table access; kernel-boot vs current-proc split per is_kernel_boot().
    unsafe {
        let idx = fd_token_idx(token);
        if idx >= VFS_MAX_FDS {
            return Err(Errno::BadFd);
        }
        let fd = fd_get(idx);
        if !fd.used || (fd.epoch & 0xFFF) != fd_token_epoch(token) {
            return Err(Errno::BadFd);
        }
        Ok(idx)
    }
}

/// # Safety
///
/// Caller contract: same fd-owning-context rule as alloc_fd; token must be a
/// live fd token (revalidated internally via fd_check).
pub(crate) unsafe fn fd_check_perm(
    token: crate::fs::vfs::vnode::FdToken,
    perm: u32,
) -> KResult<usize> {
    // SAFETY: delegates to fd_check, which bounds-checks idx and epoch
    // before returning it; no raw table access happens here.
    unsafe {
        let idx = fd_check(token)?;
        let fd = fd_get(idx);
        if fd.perms & perm == 0 {
            return Err(Errno::Perm);
        }
        Ok(idx)
    }
}

/// Rename within one OnyxFS mount. Cross-mount renames (or renames that
/// touch a non-Onyx path) are rejected with ENOSYS instead of silently
/// operating on whichever mount happened to own the driver singletons.
///
/// # Safety
///
/// Caller contract: both paths are absolute and kernel-owned (parsed by the
/// syscall layer). `with_mount` holds FS_LOCK and swaps the onyxfs singleton
/// into the target mount's context for the duration of the rename; paths
/// longer than the 256-byte user-path limit are rejected with ERANGE.
pub unsafe fn rename(old_path: &[u8], new_path: &[u8]) -> KResult<()> {
    // SAFETY: resolve_path/with_mount uphold the FS_LOCK contract; buffers
    // are fixed-size stack arrays written before being read.
    unsafe {
        let (fs_a, rel_a, slot_a) = resolve_path(old_path)?;
        let (fs_b, rel_b, slot_b) = resolve_path(new_path)?;
        if fs_a != Fs::Onyx || fs_b != Fs::Onyx || slot_a != slot_b {
            return Err(Errno::NoSys);
        }
        let mut buf_a = [0u8; 256];
        let mut buf_b = [0u8; 256];
        if slot_a >= MAX_MOUNTS {
            // Root fs: the original absolute paths are already onyxfs-shaped.
            with_mount(slot_a, || crate::fs::onyxfs::rename(old_path, new_path))
        } else {
            let a = rel_to_abs(rel_a, &mut buf_a).ok_or(Errno::Range)?;
            let b = rel_to_abs(rel_b, &mut buf_b).ok_or(Errno::Range)?;
            with_mount(slot_a, || crate::fs::onyxfs::rename(a, b))
        }
    }
}

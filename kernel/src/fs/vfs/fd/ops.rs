use onyx_core::errno::{Errno, KResult};

use crate::fs::vfs::vnode::{Fs, VFS_MAX_FDS, VfsFd, fd_token_epoch, fd_token_idx};

/// # Safety
///
/// No unsafe operations; unsafe signature kept for API symmetry with the
/// other fd-table helpers. Any kernel context may call it.
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
    cloexec: false,
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
/// re-validates idx < VFS_MAX_FDS and the epoch itself.
pub(crate) unsafe fn fd_check(token: crate::fs::vfs::vnode::FdToken) -> KResult<usize> {
    // SAFETY: bounds-checks idx (< VFS_MAX_FDS) and the epoch before any
    // fd-table access; kernel-boot vs current-proc split per is_kernel_boot().
    unsafe {
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

/// # Safety
///
/// Caller contract: idx < VFS_MAX_FDS (callers obtain it from fd_check /
/// fd_check_perm); must run in the fd-owning process's syscall context or
/// kernel boot. Returns a snapshot copy, so no aliasing survives the call.
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

/// # Safety
///
/// Caller contract: idx < VFS_MAX_FDS and is the slot allocated for this
/// open (allocated by alloc_fd); runs in the fd-owning context.
pub(crate) unsafe fn fd_set(idx: usize, ino: u32, size: u32, fs: Fs, pos: u32) {
    // SAFETY: idx is the slot just claimed by alloc_fd in this context;
    // writing it cannot race with any other user of that slot.
    unsafe {
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
}

/// # Safety
///
/// Caller contract: idx < VFS_MAX_FDS, validated by fd_check/fd_check_perm
/// at the call site; runs in the fd-owning process's syscall context.
pub(crate) unsafe fn fd_update_pos(idx: usize, pos: u32) {
    // SAFETY: idx is caller-validated via fd_check upstream; writes only the
    // pos field of the owning process's fd slot.
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
/// Caller contract: forward-only pass-through to onyxfs::rename, which owns
/// the actual on-disk safety contract (journal + lock discipline there).
pub unsafe fn rename(old_path: &[u8], new_path: &[u8]) -> KResult<()> {
    // SAFETY: only marks the call unsafe-required by the onyxfs::rename
    // signature; path slices are kernel-side validated lengths. onyxfs::rename
    // performs no cross-hart locking — concurrent renames from two harts are
    // not serialized (journal only covers crash recovery).
    unsafe { crate::fs::onyxfs::rename(old_path, new_path) }
}

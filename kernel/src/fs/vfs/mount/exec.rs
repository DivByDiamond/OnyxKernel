//! Mount-aware execution context.
//!
//! The block-filesystem drivers (`onyxfs`, `fat32`) keep their mount state
//! in module singletons. A secondary mount is served by temporarily
//! activating its cached context around each filesystem operation while
//! `FS_LOCK` is held, so the singleton switch and the I/O it protects are
//! atomic with respect to every other filesystem operation in the kernel.

use crate::fs::fat32;
use crate::fs::onyxfs;
use onyx_core::errno::{Errno, KResult};

use super::{Fs, G_MOUNTS, MAX_MOUNTS, MountFs, resolve_mount};

/// Run `f` with the filesystem singleton state of `slot` active. `slot` is
/// a `G_MOUNTS` index from `resolve_mount`, or anything `>= MAX_MOUNTS`
/// (e.g. `MNT_ROOT` or `VfsFd::mnt as u8` truncated) for the root
/// filesystem, whose context is already in the singletons.
///
/// For Onyx mounts the context is snapshotted back into the mount entry on
/// exit: mount-time growth and journal recovery persist into the superblock
/// and `G_SB`/`G_JOURNAL_HEAD` may drift during operations. Fat32 context
/// is immutable after mount (all state is re-derived from the BPB).
///
/// # Safety
///
/// Must run in kernel context with SIE=0 (trap/syscall/boot): it takes
/// `FS_LOCK` and mutates the onyxfs/fat32 module singletons (see
/// `crate::sync`). `f` must not call back into VFS path dispatch that would
/// take `FS_LOCK` again (the lock is not reentrant); the dispatch sites use
/// this helper exactly once per public entry point.
pub(crate) unsafe fn with_mount<T>(slot: usize, f: impl FnOnce() -> T) -> T {
    // SAFETY: FS_LOCK is only taken here and in the boot mount paths, all
    // with SIE=0 (crate::sync invariant). The G_MOUNTS write-back only
    // touches the `ctx` field of `slot`, which the (unlocked) readers in
    // resolve_mount never inspect: path/fs stay immutable after boot.
    unsafe {
        crate::fs::FS_LOCK.lock();
        let result = if slot >= MAX_MOUNTS {
            f()
        } else {
            let pm = &raw mut G_MOUNTS;
            match (*pm)[slot].ctx {
                Some(MountFs::Onyx(ref ctx)) => {
                    let saved = onyxfs::save_ctx();
                    onyxfs::apply_ctx(ctx);
                    let r = f();
                    (*pm)[slot].ctx = Some(MountFs::Onyx(onyxfs::save_ctx()));
                    onyxfs::apply_ctx(&saved);
                    r
                }
                Some(MountFs::Fat32(ref ctx)) => {
                    let saved = fat32::save_ctx();
                    fat32::apply_ctx(ctx);
                    let r = f();
                    fat32::apply_ctx(&saved);
                    r
                }
                None => f(),
            }
        };
        crate::fs::FS_LOCK.unlock();
        result
    }
}

/// Resolve an absolute VFS path (`/...`) into `(fs, mount-relative path,
/// mount slot)`; the relative path is root-relative for the root
/// filesystem and mount-relative for a secondary mount (b"" for the mount
/// point itself).
pub(crate) unsafe fn resolve_path(path: &[u8]) -> KResult<(Fs, &[u8], usize)> {
    // SAFETY: pure slice check plus resolve_mount's documented boot-time
    // read-only contract; with_mount callers add the FS_LOCK guarantee.
    unsafe {
        if path.is_empty() || path[0] != b'/' {
            return Err(Errno::Inval);
        }
        Ok(resolve_mount(&path[1..]))
    }
}

/// Re-attach the leading '/' that `resolve_path` stripped, for backends
/// whose path entry points require absolute paths (`onyxfs::unlink`,
/// `onyxfs::rename`). Returns `None` if `buf` is too small (callers map
/// that to `Errno::Range`).
pub(crate) fn rel_to_abs<'a>(rel: &[u8], buf: &'a mut [u8]) -> Option<&'a [u8]> {
    if rel.len() + 1 > buf.len() {
        return None;
    }
    buf[0] = b'/';
    buf[1..1 + rel.len()].copy_from_slice(rel);
    Some(&buf[..1 + rel.len()])
}

/// Stat an OnyxFS target through its mount context. Returns the stat and
/// the mount slot so the caller can finish the operation (set_mode etc.)
/// under the same context via `with_mount`.
///
/// # Safety
///
/// Same contract as `with_mount` (kernel context, SIE=0). `path` must be a
/// kernel-owned absolute path (syscall layer parses user paths first).
/// Non-Onyx targets yield `Errno::NoSys`.
pub(crate) unsafe fn stat_target(path: &[u8]) -> KResult<(onyxfs::OnyfsStat, usize)> {
    // SAFETY: with_mount covers the singleton access; lookup only reads
    // blocks into its own scratch buffer.
    unsafe {
        let (fs, rel, slot) = resolve_path(path)?;
        if fs != Fs::Onyx {
            return Err(Errno::NoSys);
        }
        let st = with_mount(slot, || -> KResult<onyxfs::OnyfsStat> {
            let mut st = onyxfs::OnyfsStat::default();
            onyxfs::lookup(rel, &mut st)?;
            Ok(st)
        })?;
        Ok((st, slot))
    }
}

/// True-check that `path` resolves to an OnyxFS directory in its mount
/// context (used by sys_chdir, which previously bypassed mount resolution).
///
/// # Safety
///
/// Same contract as `stat_target`.
pub(crate) unsafe fn is_dir_target(path: &[u8]) -> KResult<u32> {
    // SAFETY: see stat_target; resolve_dir is a read-only lookup variant.
    unsafe {
        let (fs, rel, slot) = resolve_path(path)?;
        if fs != Fs::Onyx {
            return Err(Errno::NoSys);
        }
        with_mount(slot, || onyxfs::resolve_dir(rel))
    }
}

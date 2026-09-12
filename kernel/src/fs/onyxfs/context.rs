//! OnyxFS mount context save/restore.
//!
//! The driver keeps all per-mount state in `onyxfs` module singletons. To
//! serve several OnyxFS volumes from one kernel image, `vfs::with_mount`
//! swaps the singletons per operation; this module provides the snapshot
//! type and the save/apply primitives plus a mount helper that returns a
//! context without leaving the singletons pointed at the new volume.

use super::{G_DEV, G_JOURNAL_HEAD, G_LBA_BASE, G_SB, G_VERSION, OnyfsSuper, mount};
use onyx_core::errno::KResult;

/// Snapshot of the onyxfs singleton state for one mounted volume.
#[derive(Clone, Copy)]
pub struct OnyxCtx {
    pub dev: usize,
    pub lba_base: u32,
    pub version: u32,
    pub sb: OnyfsSuper,
    pub journal_head: u32,
}

/// Capture the current singleton state.
///
/// # Safety
///
/// Reads `static mut` driver globals; call only with `FS_LOCK` held (kernel
/// context, SIE=0) so no concurrent onyxfs operation can observe or modify
/// the state mid-copy.
pub unsafe fn save_ctx() -> OnyxCtx {
    // SAFETY: caller holds FS_LOCK; all fields are plain Copy loads.
    unsafe {
        OnyxCtx {
            dev: G_DEV,
            lba_base: G_LBA_BASE,
            version: G_VERSION,
            sb: G_SB,
            journal_head: G_JOURNAL_HEAD,
        }
    }
}

/// Install `ctx` into the driver globals.
///
/// # Safety
///
/// Writes `static mut` driver globals; call only with `FS_LOCK` held, and
/// only around a single filesystem operation (see `vfs::with_mount`).
pub unsafe fn apply_ctx(ctx: &OnyxCtx) {
    // SAFETY: caller holds FS_LOCK; plain stores, no pointers involved.
    unsafe {
        G_DEV = ctx.dev;
        G_LBA_BASE = ctx.lba_base;
        G_VERSION = ctx.version;
        G_SB = ctx.sb;
        G_JOURNAL_HEAD = ctx.journal_head;
    }
}

/// Mount a secondary OnyxFS volume and return its context, restoring the
/// previously active singleton state afterwards. Boot-path only (takes
/// FS_LOCK, not reentrant with `with_mount`).
///
/// # Safety
///
/// Kernel boot context, SIE=0, no other harts in FS code yet. `dev` must be
/// a valid virtio-blk device index and `lba` within its capacity.
pub unsafe fn mount_secondary(dev: usize, lba: u32) -> KResult<OnyxCtx> {
    // SAFETY: boot-time single-hart path; FS_LOCK additionally serializes
    // the save/restore against any runtime with_mount operation.
    unsafe {
        crate::fs::FS_LOCK.lock();
        let saved = save_ctx();
        let result = mount(dev, lba);
        let ctx = save_ctx();
        apply_ctx(&saved);
        crate::fs::FS_LOCK.unlock();
        result?;
        Ok(ctx)
    }
}

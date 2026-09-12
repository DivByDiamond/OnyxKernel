//! FAT32 mount context save/restore (mirror of `onyxfs::context`).
//!
//! All fields are BPB geometry derived at mount time and immutable
//! afterwards, so `with_mount` never writes the cached context back.

use super::{G_DATA_LBA, G_DEV, G_FAT_SZ, G_NUM_FATS, G_RESVD, G_ROOT_CLUSTER, G_SPC, mount};
use onyx_core::errno::KResult;

/// Snapshot of the fat32 singleton geometry for one mounted volume.
#[derive(Clone, Copy)]
pub struct FatCtx {
    pub dev: usize,
    pub spc: u32,
    pub resvd: u32,
    pub fat_sz: u32,
    pub num_fats: u32,
    pub root_cluster: u32,
    pub data_lba: u32,
}

/// Capture the current singleton geometry.
///
/// # Safety
///
/// Reads `static mut` driver globals; call only with `FS_LOCK` held (kernel
/// context, SIE=0) so no concurrent fat32 operation can observe a mid-copy
/// state.
pub unsafe fn save_ctx() -> FatCtx {
    // SAFETY: caller holds FS_LOCK; all fields are plain Copy loads.
    unsafe {
        FatCtx {
            dev: G_DEV,
            spc: G_SPC,
            resvd: G_RESVD,
            fat_sz: G_FAT_SZ,
            num_fats: G_NUM_FATS,
            root_cluster: G_ROOT_CLUSTER,
            data_lba: G_DATA_LBA,
        }
    }
}

/// Install `ctx` into the driver globals.
///
/// # Safety
///
/// Writes `static mut` driver globals; call only with `FS_LOCK` held, and
/// only around a single filesystem operation (see `vfs::with_mount`).
pub unsafe fn apply_ctx(ctx: &FatCtx) {
    // SAFETY: caller holds FS_LOCK; plain stores, no pointers involved.
    unsafe {
        G_DEV = ctx.dev;
        G_SPC = ctx.spc;
        G_RESVD = ctx.resvd;
        G_FAT_SZ = ctx.fat_sz;
        G_NUM_FATS = ctx.num_fats;
        G_ROOT_CLUSTER = ctx.root_cluster;
        G_DATA_LBA = ctx.data_lba;
    }
}

/// Mount a secondary FAT32 volume and return its context, restoring the
/// previously active singleton state afterwards. Boot-path only.
///
/// # Safety
///
/// Kernel boot context, SIE=0, no other harts in FS code yet. `dev` must be
/// a valid virtio-blk device index.
pub unsafe fn mount_secondary(dev: usize) -> KResult<FatCtx> {
    // SAFETY: boot-time single-hart path; FS_LOCK additionally serializes
    // the save/restore against any runtime with_mount operation.
    unsafe {
        crate::fs::FS_LOCK.lock();
        let saved = save_ctx();
        let result = mount(dev);
        let ctx = save_ctx();
        apply_ctx(&saved);
        crate::fs::FS_LOCK.unlock();
        result?;
        Ok(ctx)
    }
}

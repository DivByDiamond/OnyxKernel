//! Mount, persist_superblock, and inode_table_block_count — top-level
//! filesystem lifecycle entry points invoked once at boot.
use super::{
    G_BUF, G_DEV, G_LBA_BASE, G_SB, G_VERSION, ONYFS_V1, ONYFS_V2, inodes_per_block, read_block,
    write_block,
};
use crate::drivers::virtio;
use onyx_core::errno::{Errno, KResult};
use onyx_core::fmt::Arg;
use onyx_core::formats::{ONYFS_BLOCK_SIZE, OnyfsSuper, onyxfs_growth_limit, onyxfs_growth_target};

pub unsafe fn mount(dev: usize, lba_offset: u32) -> KResult<()> {
    unsafe {
        crate::kinf!(
            "onyxfs",
            "mount dev=%d lba=%d",
            Arg::from(dev as u32),
            Arg::from(lba_offset)
        );
        G_DEV = dev;
        G_LBA_BASE = lba_offset;
        {
            let pb = &raw mut G_BUF;
            read_block(0, &mut *pb)
        }?;
        let buf_view: &[u8] = &(G_BUF);
        let sb_val = OnyfsSuper::from_bytes(buf_view).ok_or(Errno::Inval)?;
        if sb_val.block_size != ONYFS_BLOCK_SIZE as u32 {
            return Err(Errno::Inval);
        }
        // Detect version from magic. v2 = ONY2, v1 = ONY1 (legacy).
        let ver = if sb_val.magic == onyx_core::formats::ONYFS_MAGIC {
            ONYFS_V2
        } else if sb_val.magic == onyx_core::formats::ONYFS_MAGIC_V1 {
            ONYFS_V1
        } else {
            return Err(Errno::Inval);
        };
        G_VERSION = ver;
        G_SB = sb_val;
        grow_to_device()?;
        // Crash recovery: replay any committed-but-unapplied journal entries
        // before the filesystem is handed to the VFS layer.
        super::journal::journal_recover()?;
        Ok(())
    }
}

/// Read the virtio-blk device capacity in 512-byte sectors from its MMIO
/// config space (read-only u64 at offset 0x100). Returns 0 for an invalid
/// device index, which disables grow-on-mount.
unsafe fn blk_capacity_sectors(dev_idx: usize) -> u64 {
    unsafe {
        let pd = virtio::dev(dev_idx);
        if pd.is_null() {
            return 0;
        }
        // SAFETY: `dev` returned a valid registered device and `base` is its
        // mapped MMIO region; config reads are side-effect-free.
        let base = (*pd).base;
        let lo = virtio::reg_r(base, 0x100) as u64;
        let hi = virtio::reg_r(base, 0x104) as u64;
        lo | (hi << 32)
    }
}

/// Grow-on-mount: if the backing block device reports more sectors than the
/// superblock's total-blocks (image built by mkimage is smaller than the
/// disk — e.g. a ~2.4 MB image on a player's HDD), extend usable space up to
/// the device size.
///
/// What the on-disk format allows (honest limits):
/// - The data-area bitmap is a SINGLE block written by mkimage at block
///   index 2, so at most `ONYFS_BITMAP_CAPACITY_BLOCKS` (32768 ≈ 128 MiB of
///   data area) can ever be addressed without an on-disk format change.
/// - Growth is additionally capped by `ONYFS_MAX_TOTAL_BLOCKS` (1 GiB).
/// - No bitmap rewrite is required: mkimage leaves every data-bitmap bit
///   beyond the built image at zero (= free), so newly exposed blocks past
///   the old end of the FS are immediately allocatable. Data blocks sit at
///   the END of the layout (after inode table / snapshot area / journal),
///   so extending the tail cannot overlap existing metadata.
///
/// The new size is persisted to the superblock immediately; if it ever needs
/// shrinking again, rebuild the image with mkimage.
unsafe fn grow_to_device() -> KResult<()> {
    unsafe {
        let sectors = blk_capacity_sectors(G_DEV);
        if sectors == 0 {
            return Ok(());
        }
        // One OnyxFS block (4096 bytes) = 8 × 512-byte sectors.
        let dev_blocks = sectors / 8;
        let sb_ptr = &raw const G_SB;
        let old_total = (*sb_ptr).total_blocks;
        if dev_blocks <= old_total as u64 {
            return Ok(());
        }
        if let Some(new_total) =
            onyxfs_growth_target(old_total, (*sb_ptr).data_blocks_start, dev_blocks)
        {
            G_SB.total_blocks = new_total;
            persist_superblock()?;
            crate::kinf!(
                "onyxfs",
                "grown %d -> %d blocks (+%d free)",
                Arg::from(old_total),
                Arg::from(new_total),
                Arg::from(new_total - old_total)
            );
        }
        let limit = onyxfs_growth_limit((*sb_ptr).data_blocks_start);
        if dev_blocks > limit {
            crate::kwrn!(
                "onyxfs",
                "device %d blocks exceeds growth cap %d (single-block data bitmap)",
                Arg::from(dev_blocks),
                Arg::from(limit)
            );
        }
        Ok(())
    }
}

/// Persist the in-memory superblock back to disk block 0.
pub(super) unsafe fn persist_superblock() -> KResult<()> {
    unsafe {
        let bytes = (G_SB).to_bytes();
        let pb = &raw mut G_BUF;
        // Zero the block so stale data beyond the superblock doesn't leak.
        for b in (*pb).iter_mut() {
            *b = 0;
        }
        (&mut *pb)[..bytes.len()].copy_from_slice(&bytes);
        write_block(0, &*pb)
    }
}

/// Number of inode-table blocks occupied by the current filesystem.
#[inline]
pub(super) unsafe fn inode_table_block_count() -> u32 {
    unsafe {
        let ipb = inodes_per_block() as u32;
        let cnt = (G_SB).inode_count;
        if cnt == 0 { 1 } else { cnt.div_ceil(ipb) }
    }
}

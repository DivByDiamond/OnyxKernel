//! Mount, persist_superblock, and inode_table_block_count — top-level
//! filesystem lifecycle entry points invoked once at boot.
use super::{
    inodes_per_block, read_block, write_block, G_BUF, G_DEV, G_LBA_BASE, G_SB, G_VERSION, ONYFS_V1,
    ONYFS_V2,
};
use onyx_core::errno::{Errno, KResult};
use onyx_core::formats::{OnyfsSuper, ONYFS_BLOCK_SIZE};

pub unsafe fn mount(dev: usize, lba_offset: u32) -> KResult<()> {
    *(&raw mut G_DEV) = dev;
    *(&raw mut G_LBA_BASE) = lba_offset;
    {
        let pb = &raw mut G_BUF;
        read_block(0, &mut *pb)
    }?;
    let buf_view: &[u8] = &(*(&raw const G_BUF));
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
    *(&raw mut G_VERSION) = ver;
    *(&raw mut G_SB) = sb_val;
    // Crash recovery: replay any committed-but-unapplied journal entries
    // before the filesystem is handed to the VFS layer.
    super::journal::journal_recover()?;
    Ok(())
}

/// Persist the in-memory superblock back to disk block 0.
pub(super) unsafe fn persist_superblock() -> KResult<()> {
    let bytes = (*(&raw const G_SB)).to_bytes();
    let pb = &raw mut G_BUF;
    // Zero the block so stale data beyond the superblock doesn't leak.
    for b in (*pb).iter_mut() {
        *b = 0;
    }
    for i in 0..bytes.len() {
        (*pb)[i] = bytes[i];
    }
    write_block(0, &*pb)
}

/// Number of inode-table blocks occupied by the current filesystem.
#[inline]
pub(super) unsafe fn inode_table_block_count() -> u32 {
    let ipb = inodes_per_block() as u32;
    let cnt = (*(&raw const G_SB)).inode_count;
    if cnt == 0 {
        1
    } else {
        (cnt + ipb - 1) / ipb
    }
}

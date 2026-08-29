//! OnyxFS truncation: free file data blocks, truncate to zero and the
//! POSIX truncate-to-length shrink path. Block-range growth (extend) is
//! delegated to [`super::extend`].

use crate::fs::onyxfs::alloc::free_data_block;
use crate::fs::onyxfs::inode::{read_inode, write_inode};
use crate::fs::onyxfs::journal::{journal_commit, journal_log};
use crate::fs::onyxfs::write::check_v2;
use crate::fs::onyxfs::{G_BUF, read_block, write_block};
use crate::srv::timer;
use onyx_core::errno::KResult;
use onyx_core::formats::{ONYFS_BLOCK_SIZE, ONYFS_DIRECT_BLKS, OnyfsInode};

use super::extend::extend_to_length;

/// Free all data blocks held by `inode`, both direct and indirect.
/// Used by `truncate(ino)` (zero-length) and as a building block for
/// partial truncation. Does NOT touch the inode itself — caller is
/// responsible for updating inode.blocks/indirect/size.
///
/// # Safety
///
/// Caller must not invoke onyxfs operations concurrently from multiple harts
/// (free_data_block uses the shared G_BUF scratch global). All block numbers
/// come from a previously read inode and are validated by free_data_block.
unsafe fn free_all_blocks(inode: &mut OnyfsInode) -> KResult<()> {
    // SAFETY: only calls free_data_block/free_indirect_block/
    // free_double_indirect, whose own contracts cover the raw access.
    unsafe {
        for &blk in inode.blocks.iter() {
            if blk != 0 {
                free_data_block(blk)?;
            }
        }
        inode.blocks = [0; ONYFS_DIRECT_BLKS];

        if inode.indirect != 0 {
            free_indirect_block(inode.indirect)?;
            inode.indirect = 0;
        }

        if inode.double_indirect != 0 {
            free_double_indirect(inode.double_indirect)?;
            inode.double_indirect = 0;
        }
        Ok(())
    }
}

/// Free every block referenced by a single-indirect block, then the
/// indirect block itself.
///
/// # Safety
///
/// Caller must not invoke onyxfs operations concurrently from multiple harts
/// (shared G_BUF scratch global). `ind_blk` must be an indirect block number
/// from a valid inode; entry reads stay inside the 4 KB block buffer.
unsafe fn free_indirect_block(ind_blk: u32) -> KResult<()> {
    // SAFETY: single-threaded onyxfs exclusion (see # Safety); `pb` is the
    // module-global G_BUF, valid for a full block; entry loop is bounded.
    unsafe {
        let pb = &raw mut G_BUF;
        read_block(ind_blk, &mut *pb)?;
        for i in 0..entries_per_block() {
            let blk = ind_entry(&*pb, i);
            if blk != 0 {
                free_data_block(blk)?;
            }
        }
        free_data_block(ind_blk)
    }
}

/// Free every block referenced by a double-indirect block, both levels,
/// then the double-indirect block itself.
///
/// # Safety
///
/// Caller must not invoke onyxfs operations concurrently from multiple harts
/// (shared G_BUF scratch global). `dind_blk` must be a double-indirect block
/// number from a valid inode; entry reads stay inside the block buffer.
unsafe fn free_double_indirect(dind_blk: u32) -> KResult<()> {
    // SAFETY: single-threaded onyxfs exclusion (see # Safety); `pb` is the
    // module-global G_BUF, valid for a full block; entry loop is bounded.
    unsafe {
        let pb = &raw mut G_BUF;
        for i in 0..entries_per_block() {
            read_block(dind_blk, &mut *pb)?;
            let ind_blk = ind_entry(&*pb, i);
            if ind_blk != 0 {
                free_indirect_block(ind_blk)?;
            }
        }
        free_data_block(dind_blk)
    }
}

/// Truncate the file to exactly 0 bytes — frees all data blocks.
///
/// # Safety
///
/// Caller must not invoke onyxfs operations concurrently from multiple harts
/// (shared G_BUF, journal head, inode slots). `ino` must be a valid inode.
pub unsafe fn truncate(ino: u32) -> KResult<()> {
    // SAFETY: single-threaded onyxfs exclusion (see # Safety); raw access is
    // delegated to the bounds-checked read_inode/free_all_blocks helpers.
    unsafe {
        check_v2()?;
        let mut inode = OnyfsInode::default();
        read_inode(ino, &mut inode)?;
        free_all_blocks(&mut inode)?;
        inode.size = 0;
        inode.mtime = timer::jiffies();
        write_inode(ino, &inode)?;
        journal_commit()?;
        Ok(())
    }
}

/// Truncate the file to exactly `length` bytes (POSIX truncate/ftruncate).
///
/// Three cases:
///   - length == 0: free all data blocks (delegates to `truncate(ino)`).
///   - length < current_size: free any block whose block-index is fully
///     past `length`, and update inode.size. Partial-trailing block is
///     kept as-is (its bytes past `length` are simply not visible).
///   - length > current_size: allocate new zero-filled blocks to cover the
///     extended range (see `extend::extend_to_length`). This guarantees
///     that read() sees zeros past the old EOF (POSIX semantics).
///
/// Block-index math (bs = ONYFS_BLOCK_SIZE = 4096):
///   block_index = floor(offset / bs)
///   blocks 0..10 are direct; blocks 10..(10 + bs/4) are in the single
///   indirect block.
///
/// # Safety
///
/// Caller must not invoke onyxfs operations concurrently from multiple harts
/// (shared G_BUF, journal head, inode slots). `ino` must be a valid inode.
pub unsafe fn truncate_to_length(ino: u32, length: u64) -> KResult<()> {
    // SAFETY: single-threaded onyxfs exclusion (see # Safety); raw access is
    // delegated to the bounds-checked read_inode/shrink/extend helpers.
    unsafe {
        check_v2()?;
        let mut inode = OnyfsInode::default();
        read_inode(ino, &mut inode)?;
        let cur_size = inode.size;

        if length == cur_size {
            return Ok(());
        }
        if length == 0 {
            return truncate(ino);
        }

        if length < cur_size {
            shrink(&mut inode, length)?;
        } else {
            extend_to_length(&mut inode, cur_size, length)?;
        }

        inode.size = length;
        inode.mtime = timer::jiffies();
        write_inode(ino, &inode)?;
        journal_commit()?;
        Ok(())
    }
}

/// Shrink: free direct blocks and indirect entries past the last block
/// needed to hold `length` bytes.
///
/// # Safety
///
/// Caller must not invoke onyxfs operations concurrently from multiple harts
/// (shared G_BUF scratch global). `length` must be nonzero and < the inode's
/// current size; indirect entry reads are bounded by entries_per_block().
unsafe fn shrink(inode: &mut OnyfsInode, length: u64) -> KResult<()> {
    // SAFETY: single-threaded onyxfs exclusion (see # Safety); `pb` is the
    // module-global G_BUF; indirect entry indices are bounded by the loop.
    unsafe {
        let bs = ONYFS_BLOCK_SIZE as u64;
        // Block at index `last_needed` partially contains the byte at
        // offset `length-1`, so we keep blocks [0..=last_needed].
        let last_needed: u64 = (length - 1) / bs;
        if last_needed < ONYFS_DIRECT_BLKS as u64 {
            // Free direct blocks past last_needed.
            for i in (last_needed as usize + 1)..ONYFS_DIRECT_BLKS {
                if inode.blocks[i] != 0 {
                    free_data_block(inode.blocks[i])?;
                    inode.blocks[i] = 0;
                }
            }
            // The indirect block is now entirely unneeded.
            if inode.indirect != 0 {
                free_indirect_block(inode.indirect)?;
                inode.indirect = 0;
            }
        } else if inode.indirect != 0 {
            // last_needed is in the indirect range. Free indirect entries
            // past (last_needed - ONYFS_DIRECT_BLKS).
            let ind_last_needed = (last_needed - ONYFS_DIRECT_BLKS as u64) as usize;
            let pb = &raw mut G_BUF;
            read_block(inode.indirect, &mut *pb)?;
            for i in (ind_last_needed + 1)..entries_per_block() {
                let blk = ind_entry(&*pb, i);
                if blk != 0 {
                    free_data_block(blk)?;
                    set_ind_entry(&mut *pb, i, 0);
                }
            }
            journal_log(inode.indirect, &*pb)?;
            write_block(inode.indirect, &*pb)?;
        }
        // Note: double_indirect handling skipped — OnyxFS only grows files
        // through direct + single-indirect blocks, so no file ever has
        // double_indirect set here.
        Ok(())
    }
}

pub(super) fn entries_per_block() -> usize {
    ONYFS_BLOCK_SIZE / 4
}

/// Read the `idx`-th u32 entry of an indirect block image.
pub(super) fn ind_entry(buf: &[u8], idx: usize) -> u32 {
    let off = idx * 4;
    u32::from_le_bytes([buf[off], buf[off + 1], buf[off + 2], buf[off + 3]])
}

/// Write the `idx`-th u32 entry of an indirect block image.
pub(super) fn set_ind_entry(buf: &mut [u8], idx: usize, val: u32) {
    let off = idx * 4;
    buf[off..off + 4].copy_from_slice(&val.to_le_bytes());
}

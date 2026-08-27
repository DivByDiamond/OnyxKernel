//! OnyxFS truncate-to-length extend path: allocate zero-filled data
//! blocks to cover the byte range [cur_size, length) so that reads past
//! the old EOF return zeros (POSIX truncate(2) semantics).

use super::truncate::{entries_per_block, ind_entry, set_ind_entry};
use crate::fs::onyxfs::alloc::alloc_data_block;
use crate::fs::onyxfs::journal::journal_log;
use crate::fs::onyxfs::{G_BUF, read_block, write_block};
use onyx_core::errno::KResult;
use onyx_core::formats::{ONYFS_BLOCK_SIZE, ONYFS_DIRECT_BLKS, OnyfsInode};

/// Ensure blocks [first_needed..=last_needed] exist on `inode`, freshly
/// allocated ones zero-filled. `cur_size` is only used to compute the
/// first block that may need allocation.
pub(super) unsafe fn extend_to_length(
    inode: &mut OnyfsInode,
    cur_size: u64,
    length: u64,
) -> KResult<()> {
    unsafe {
        let bs = ONYFS_BLOCK_SIZE as u64;
        let first_new_block: u64 = if cur_size == 0 {
            0
        } else {
            (cur_size - 1) / bs + 1
        };
        let last_block_needed: u64 = (length - 1) / bs;

        let mut ind_buf = [0u8; ONYFS_BLOCK_SIZE];
        let mut ind_dirty = false;
        if inode.indirect != 0 {
            read_block(inode.indirect, &mut ind_buf)?;
        }

        let mut cur_idx = first_new_block;
        while cur_idx <= last_block_needed {
            if cur_idx < ONYFS_DIRECT_BLKS as u64 {
                let i = cur_idx as usize;
                if inode.blocks[i] == 0 {
                    inode.blocks[i] = alloc_zero_block()?;
                }
            } else {
                let ind_idx = (cur_idx - ONYFS_DIRECT_BLKS as u64) as usize;
                if ind_idx >= entries_per_block() {
                    break;
                }
                if inode.indirect == 0 {
                    inode.indirect = alloc_data_block()?;
                    ind_buf = [0u8; ONYFS_BLOCK_SIZE];
                    ind_dirty = true;
                }
                if ind_entry(&ind_buf, ind_idx) == 0 {
                    let new_blk = alloc_zero_block()?;
                    set_ind_entry(&mut ind_buf, ind_idx, new_blk);
                    ind_dirty = true;
                }
            }
            cur_idx += 1;
        }
        if ind_dirty {
            journal_log(inode.indirect, &ind_buf)?;
            write_block(inode.indirect, &ind_buf)?;
        }
        Ok(())
    }
}

/// Allocate a data block, zero-fill it and persist it through the journal.
unsafe fn alloc_zero_block() -> KResult<u32> {
    unsafe {
        let blk = alloc_data_block()?;
        let pb = &raw mut G_BUF;
        for b in (*pb).iter_mut() {
            *b = 0;
        }
        journal_log(blk, &*pb)?;
        write_block(blk, &*pb)?;
        Ok(blk)
    }
}

use super::super::alloc::{add_dirent, alloc_data_block, alloc_inode, free_data_block};
use super::super::inode::{read_inode, write_inode};
use super::super::journal::{journal_commit, journal_log};
use super::super::{G_BUF, read_block, write_block};
use super::check_v2;
use crate::srv::timer;
use onyx_core::errno::{Errno, KResult};
use onyx_core::formats::{ONYFS_BLOCK_SIZE, ONYFS_DIRECT_BLKS, ONYFS_NAME_MAX, OnyfsInode};

pub unsafe fn create(dir_ino: u32, name: &[u8], mode: u32) -> KResult<u32> {
    check_v2()?;
    if name.is_empty() || name.len() > ONYFS_NAME_MAX {
        return Err(Errno::Inval);
    }
    let new_ino = alloc_inode()?;
    let now = *(&raw const timer::G_JIFFIES);
    let inode = OnyfsInode {
        mode,
        size: 0,
        uid: 0,
        gid: 0,
        nlink: 1,
        blocks: [0; ONYFS_DIRECT_BLKS],
        indirect: 0,
        double_indirect: 0,
        crtime: now,
        mtime: now,
        atime: now,
        ctime: now,
        flags: 0,
        reserved: 0,
    };
    write_inode(new_ino, &inode)?;
    add_dirent(dir_ino, name, new_ino, 8)?;
    journal_commit()?;
    Ok(new_ino)
}

/// Free all data blocks held by `inode`, both direct and indirect.
/// Used by `truncate(ino)` (zero-length) and as a building block for
/// partial truncation. Does NOT touch the inode itself — caller is
/// responsible for updating inode.blocks/indirect/size.
pub(super) unsafe fn free_all_blocks(inode: &mut OnyfsInode) -> KResult<()> {
    for &blk in inode.blocks.iter() {
        if blk != 0 {
            free_data_block(blk)?;
        }
    }
    inode.blocks = [0; ONYFS_DIRECT_BLKS];

    if inode.indirect != 0 {
        let pb = &raw mut G_BUF;
        read_block(inode.indirect, &mut *pb)?;
        for i in 0..ONYFS_BLOCK_SIZE / 4 {
            let off = i * 4;
            let blk =
                u32::from_le_bytes([(*pb)[off], (*pb)[off + 1], (*pb)[off + 2], (*pb)[off + 3]]);
            if blk != 0 {
                free_data_block(blk)?;
            }
        }
        free_data_block(inode.indirect)?;
        inode.indirect = 0;
    }

    if inode.double_indirect != 0 {
        let pb = &raw mut G_BUF;
        for i in 0..ONYFS_BLOCK_SIZE / 4 {
            read_block(inode.double_indirect, &mut *pb)?;
            let off = i * 4;
            let ind_blk =
                u32::from_le_bytes([(*pb)[off], (*pb)[off + 1], (*pb)[off + 2], (*pb)[off + 3]]);
            if ind_blk != 0 {
                read_block(ind_blk, &mut *pb)?;
                for j in 0..ONYFS_BLOCK_SIZE / 4 {
                    let off2 = j * 4;
                    let blk = u32::from_le_bytes([
                        (*pb)[off2],
                        (*pb)[off2 + 1],
                        (*pb)[off2 + 2],
                        (*pb)[off2 + 3],
                    ]);
                    if blk != 0 {
                        free_data_block(blk)?;
                    }
                }
                free_data_block(ind_blk)?;
            }
        }
        free_data_block(inode.double_indirect)?;
        inode.double_indirect = 0;
    }
    Ok(())
}

/// Truncate the file to exactly 0 bytes — frees all data blocks.
pub unsafe fn truncate(ino: u32) -> KResult<()> {
    check_v2()?;
    let mut inode = OnyfsInode {
        mode: 0,
        size: 0,
        uid: 0,
        gid: 0,
        nlink: 0,
        blocks: [0; ONYFS_DIRECT_BLKS],
        indirect: 0,
        double_indirect: 0,
        crtime: 0,
        mtime: 0,
        atime: 0,
        ctime: 0,
        flags: 0,
        reserved: 0,
    };
    read_inode(ino, &mut inode)?;
    free_all_blocks(&mut inode)?;
    inode.size = 0;
    inode.mtime = *(&raw const timer::G_JIFFIES);
    write_inode(ino, &inode)?;
    journal_commit()?;
    Ok(())
}

/// Truncate the file to exactly `length` bytes (POSIX truncate/ftruncate).
///
/// Three cases:
///   - length == 0: free all data blocks (delegates to `truncate(ino)`).
///   - length < current_size: free any block whose block-index is fully
///     past `length`, and update inode.size. Partial-trailing block is
///     kept as-is (its bytes past `length` are simply not visible).
///   - length > current_size: allocate new zero-filled blocks to cover the
///     extended range. Reads past the allocated range would otherwise
///     return short reads; allocating the blocks guarantees that read()
///     sees zeros (matching POSIX truncate(2) semantics).
///
/// Block-index math (bs = ONYFS_BLOCK_SIZE = 4096):
///   block_index = floor(offset / bs)
///   blocks 0..10 are direct; blocks 10..(10 + bs/4) are in the single
///   indirect block.
pub unsafe fn truncate_to_length(ino: u32, length: u64) -> KResult<()> {
    check_v2()?;
    let mut inode = OnyfsInode {
        mode: 0,
        size: 0,
        uid: 0,
        gid: 0,
        nlink: 0,
        blocks: [0; ONYFS_DIRECT_BLKS],
        indirect: 0,
        double_indirect: 0,
        crtime: 0,
        mtime: 0,
        atime: 0,
        ctime: 0,
        flags: 0,
        reserved: 0,
    };
    read_inode(ino, &mut inode)?;
    let cur_size = inode.size;

    if length == cur_size {
        return Ok(());
    }
    if length == 0 {
        return truncate(ino);
    }

    let bs = ONYFS_BLOCK_SIZE as u64;

    if length < cur_size {
        // Shrink. Compute the last block index that should still hold data.
        // Block at index `last_needed` partially contains the byte at
        // offset `length-1`, so we keep blocks [0..=last_needed].
        let last_needed: u64 = (length - 1) / bs;
        // Free direct blocks past last_needed.
        if last_needed < ONYFS_DIRECT_BLKS as u64 {
            for i in (last_needed as usize + 1)..ONYFS_DIRECT_BLKS {
                if inode.blocks[i] != 0 {
                    free_data_block(inode.blocks[i])?;
                    inode.blocks[i] = 0;
                }
            }
            // Also free the indirect block if it's now unneeded.
            if inode.indirect != 0 {
                let pb = &raw mut G_BUF;
                read_block(inode.indirect, &mut *pb)?;
                for i in 0..ONYFS_BLOCK_SIZE / 4 {
                    let off = i * 4;
                    let blk = u32::from_le_bytes([
                        (*pb)[off],
                        (*pb)[off + 1],
                        (*pb)[off + 2],
                        (*pb)[off + 3],
                    ]);
                    if blk != 0 {
                        free_data_block(blk)?;
                    }
                }
                free_data_block(inode.indirect)?;
                inode.indirect = 0;
            }
        } else {
            // last_needed is in the indirect range. Free indirect entries
            // past (last_needed - ONYFS_DIRECT_BLKS).
            let ind_last_needed = (last_needed - ONYFS_DIRECT_BLKS as u64) as usize;
            if inode.indirect != 0 {
                let pb = &raw mut G_BUF;
                read_block(inode.indirect, &mut *pb)?;
                for i in (ind_last_needed + 1)..(ONYFS_BLOCK_SIZE / 4) {
                    let off = i * 4;
                    let blk = u32::from_le_bytes([
                        (*pb)[off],
                        (*pb)[off + 1],
                        (*pb)[off + 2],
                        (*pb)[off + 3],
                    ]);
                    if blk != 0 {
                        free_data_block(blk)?;
                        // Zero out the entry.
                        (*pb)[off] = 0;
                        (*pb)[off + 1] = 0;
                        (*pb)[off + 2] = 0;
                        (*pb)[off + 3] = 0;
                    }
                }
                journal_log(inode.indirect, &*pb)?;
                write_block(inode.indirect, &*pb)?;
            }
        }
        // Note: double_indirect handling skipped — OnyxFS write.rs only
        // supports direct + single-indirect, so no file ever grows past
        // that range. We leave double_indirect alone for safety.
    } else {
        // Extend. Allocate zero-filled blocks to cover [cur_size, length).
        let first_new_block: u64 = if cur_size == 0 { 0 } else { (cur_size - 1) / bs + 1 };
        let last_block_needed: u64 = (length - 1) / bs;
        // For each block index in [first_new_block..=last_block_needed],
        // ensure a data block exists.
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
                    let blk = alloc_data_block()?;
                    // Zero-fill the new block.
                    let pb = &raw mut G_BUF;
                    for b in (*pb).iter_mut() {
                        *b = 0;
                    }
                    journal_log(blk, &*pb)?;
                    write_block(blk, &*pb)?;
                    inode.blocks[i] = blk;
                }
            } else {
                let ind_idx = (cur_idx - ONYFS_DIRECT_BLKS as u64) as usize;
                if ind_idx >= ONYFS_BLOCK_SIZE / 4 {
                    break;
                }
                if inode.indirect == 0 {
                    let ind_blk = alloc_data_block()?;
                    inode.indirect = ind_blk;
                    for b in ind_buf.iter_mut() {
                        *b = 0;
                    }
                    ind_dirty = true;
                }
                let entry_off = ind_idx * 4;
                let blk = u32::from_le_bytes([
                    ind_buf[entry_off],
                    ind_buf[entry_off + 1],
                    ind_buf[entry_off + 2],
                    ind_buf[entry_off + 3],
                ]);
                if blk == 0 {
                    let new_blk = alloc_data_block()?;
                    let bytes = new_blk.to_le_bytes();
                    ind_buf[entry_off] = bytes[0];
                    ind_buf[entry_off + 1] = bytes[1];
                    ind_buf[entry_off + 2] = bytes[2];
                    ind_buf[entry_off + 3] = bytes[3];
                    ind_dirty = true;
                    // Zero-fill the new block.
                    let pb = &raw mut G_BUF;
                    for b in (*pb).iter_mut() {
                        *b = 0;
                    }
                    journal_log(new_blk, &*pb)?;
                    write_block(new_blk, &*pb)?;
                }
            }
            cur_idx += 1;
        }
        if ind_dirty {
            journal_log(inode.indirect, &ind_buf)?;
            write_block(inode.indirect, &ind_buf)?;
        }
    }

    inode.size = length;
    inode.mtime = *(&raw const timer::G_JIFFIES);
    write_inode(ino, &inode)?;
    journal_commit()?;
    Ok(())
}

use super::super::inode;
use super::super::journal::journal_log;
use super::super::{
    read_block, write_block, G_BUF, G_SB,
};
use onyx_core::errno::{Errno, KResult};
use onyx_core::formats::{ONYFS_BLOCK_SIZE, ONYFS_DIRECT_BLKS, OnyfsInode};

pub unsafe fn alloc_data_block() -> KResult<u32> {
    let bm_blk = (*(&raw const G_SB)).data_bitmap_start;
    let pb = &raw mut G_BUF;
    read_block(bm_blk, &mut *pb)?;
    for byte_idx in 0..ONYFS_BLOCK_SIZE {
        if (*pb)[byte_idx] == 0xFF {
            continue;
        }
        for bit in 0..8u32 {
            if (*pb)[byte_idx] & (1 << bit) == 0 {
                (*pb)[byte_idx] |= 1 << bit;
                let bit_index = (byte_idx as u32) * 8 + bit;
                journal_log(bm_blk, &*pb)?;
                write_block(bm_blk, &*pb)?;
                return Ok((*(&raw const G_SB)).data_blocks_start + bit_index);
            }
        }
    }
    Err(Errno::NoSpace)
}

pub unsafe fn free_data_block(blk_num: u32) -> KResult<()> {
    let bm_blk = (*(&raw const G_SB)).data_bitmap_start;
    let data_start = (*(&raw const G_SB)).data_blocks_start;
    // Bug #21 fix (also listed in Phase 5 security): bounds-check the
    // computed bit_index. The previous code used wrapping_sub and could
    // produce a huge bit_index when blk_num < data_start, leading to an
    // out-of-bounds write into the bitmap buffer (and from there, into
    // kernel memory via the journaled write_block).
    if blk_num < data_start {
        return Err(Errno::Inval);
    }
    let bit_index = (blk_num - data_start) as usize;
    let max_bits = ONYFS_BLOCK_SIZE * 8;
    if bit_index >= max_bits {
        return Err(Errno::Inval);
    }
    let byte_idx = bit_index / 8;
    let bit = (bit_index % 8) as u8;
    let pb = &raw mut G_BUF;
    read_block(bm_blk, &mut *pb)?;
    (*pb)[byte_idx] &= !(1 << bit);
    journal_log(bm_blk, &*pb)?;
    write_block(bm_blk, &*pb)?;
    Ok(())
}

pub unsafe fn alloc_inode() -> KResult<u32> {
    const INODE_BITMAP_BLK: u32 = 1;
    let pb = &raw mut G_BUF;
    read_block(INODE_BITMAP_BLK, &mut *pb)?;
    for byte_idx in 0..ONYFS_BLOCK_SIZE {
        if (*pb)[byte_idx] == 0xFF {
            continue;
        }
        for bit in 0..8u32 {
            if (*pb)[byte_idx] & (1 << bit) == 0 {
                (*pb)[byte_idx] |= 1 << bit;
                let bit_index = (byte_idx as u32) * 8 + bit;
                journal_log(INODE_BITMAP_BLK, &*pb)?;
                write_block(INODE_BITMAP_BLK, &*pb)?;
                return Ok(bit_index + 1);
            }
        }
    }
    Err(Errno::NoSpace)
}

/// Release an inode and every block it references.
///
/// Bug #20 fix: previously `unlink` only zeroed the dirent's `inode` field
/// and never freed the inode itself, its data blocks, or its indirect
/// blocks. Every unlink therefore leaked an inode + all of its data blocks
/// forever — a long-running system would exhaust the inode table and the
/// data bitmap without any way to recover the space.
///
/// This function:
///   1. Reads the inode.
///   2. Frees every direct block (blocks[0..ONYFS_DIRECT_BLKS]).
///   3. If `indirect` is set, walks the single-indirect block and frees
///      every block it references, then frees the indirect block itself.
///   4. If `double_indirect` is set, walks the double-indirect block
///      (one level deep) and frees every indirect + data block referenced.
///   5. Zeroes the on-disk inode entry.
///   6. Clears the inode bitmap bit.
///
/// All metadata writes go through the journal so the reclaim is crash-safe.
pub unsafe fn free_inode(ino: u32) -> KResult<()> {
    if ino == 0 {
        return Err(Errno::Inval);
    }

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
    inode::read_inode(ino, &mut inode)?;

    // Free direct blocks.
    for &blk in inode.blocks.iter() {
        if blk != 0 {
            free_data_block(blk)?;
        }
    }

    // Free single-indirect block and every block it points at.
    // We use a stack-local buffer so we don't clobber G_BUF (which
    // free_data_block uses internally for the data-bitmap update).
    if inode.indirect != 0 {
        let mut ind_buf = [0u8; ONYFS_BLOCK_SIZE];
        read_block(inode.indirect, &mut ind_buf)?;
        for i in 0..ONYFS_BLOCK_SIZE / 4 {
            let off = i * 4;
            let blk = u32::from_le_bytes([
                ind_buf[off],
                ind_buf[off + 1],
                ind_buf[off + 2],
                ind_buf[off + 3],
            ]);
            if blk != 0 {
                free_data_block(blk)?;
            }
        }
        free_data_block(inode.indirect)?;
    }

    // Free double-indirect block and every indirect / data block it points
    // at (single-level traversal of the indirect blocks it references).
    if inode.double_indirect != 0 {
        let mut dbl_buf = [0u8; ONYFS_BLOCK_SIZE];
        read_block(inode.double_indirect, &mut dbl_buf)?;
        for i in 0..ONYFS_BLOCK_SIZE / 4 {
            let off = i * 4;
            let ind_blk = u32::from_le_bytes([
                dbl_buf[off],
                dbl_buf[off + 1],
                dbl_buf[off + 2],
                dbl_buf[off + 3],
            ]);
            if ind_blk != 0 {
                let mut ind_buf = [0u8; ONYFS_BLOCK_SIZE];
                read_block(ind_blk, &mut ind_buf)?;
                for j in 0..ONYFS_BLOCK_SIZE / 4 {
                    let off2 = j * 4;
                    let blk = u32::from_le_bytes([
                        ind_buf[off2],
                        ind_buf[off2 + 1],
                        ind_buf[off2 + 2],
                        ind_buf[off2 + 3],
                    ]);
                    if blk != 0 {
                        free_data_block(blk)?;
                    }
                }
                free_data_block(ind_blk)?;
            }
        }
        free_data_block(inode.double_indirect)?;
    }

    // Zero the on-disk inode entry so a stale read can't resurrect it.
    let zeroed = OnyfsInode {
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
    inode::write_inode(ino, &zeroed)?;

    // Clear the inode bitmap bit. Inode N corresponds to bit (N-1) in the
    // inode bitmap block (alloc_inode() returns bit_index + 1).
    const INODE_BITMAP_BLK: u32 = 1;
    let idx = (ino as usize).saturating_sub(1);
    let byte_idx = idx / 8;
    let bit = (idx % 8) as u8;
    if byte_idx < ONYFS_BLOCK_SIZE {
        let pb = &raw mut G_BUF;
        read_block(INODE_BITMAP_BLK, &mut *pb)?;
        (*pb)[byte_idx] &= !(1 << bit);
        journal_log(INODE_BITMAP_BLK, &*pb)?;
        write_block(INODE_BITMAP_BLK, &*pb)?;
    }
    Ok(())
}

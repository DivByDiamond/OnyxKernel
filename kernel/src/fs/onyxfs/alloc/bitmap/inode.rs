use super::super::super::inode;
use super::super::journal::journal_log;
use super::super::{read_block, write_block, G_BUF};
use super::free_data_block;
use onyx_core::errno::{Errno, KResult};
use onyx_core::formats::{OnyfsInode, ONYFS_BLOCK_SIZE, ONYFS_DIRECT_BLKS};

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

    for &blk in inode.blocks.iter() {
        if blk != 0 {
            free_data_block(blk)?;
        }
    }

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

use super::super::inode;
use super::super::journal::journal_log;
use super::super::{read_block, write_block, G_BUF, G_SB};
use super::bitmap::free_data_block;
use onyx_core::errno::{Errno, KResult};
use onyx_core::formats::{OnyfsInode, ONYFS_BLOCK_SIZE, ONYFS_DIRECT_BLKS};

pub unsafe fn free_inode(ino: u32) -> KResult<()> {
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
    }

    const INODE_BITMAP_BLK: u32 = 1;
    let bit_index = (ino as usize).saturating_sub(1);
    let byte_idx = bit_index / 8;
    let bit = (bit_index % 8) as u8;
    let pb = &raw mut G_BUF;
    read_block(INODE_BITMAP_BLK, &mut *pb)?;
    (*pb)[byte_idx] &= !(1 << bit);
    journal_log(INODE_BITMAP_BLK, &*pb)?;
    write_block(INODE_BITMAP_BLK, &*pb)?;

    Ok(())
}

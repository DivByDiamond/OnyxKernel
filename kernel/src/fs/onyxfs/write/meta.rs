use super::super::alloc::{add_dirent, alloc_inode, free_data_block};
use super::super::inode::{read_inode, write_inode};
use super::super::journal::{journal_commit, journal_log};
use super::super::{read_block, write_block, G_BUF};
use super::check_v2;
use crate::srv::timer;
use onyx_core::errno::{Errno, KResult};
use onyx_core::formats::{OnyfsInode, ONYFS_BLOCK_SIZE, ONYFS_DIRECT_BLKS, ONYFS_NAME_MAX};

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

    inode.size = 0;
    inode.blocks = [0; ONYFS_DIRECT_BLKS];
    inode.indirect = 0;
    inode.double_indirect = 0;
    write_inode(ino, &inode)?;
    journal_commit()?;
    Ok(())
}

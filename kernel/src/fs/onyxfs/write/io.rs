use super::super::alloc::alloc_data_block;
use super::super::inode::{read_inode, write_inode};
use super::super::journal::{journal_commit, journal_log};
use super::super::{read_block, write_block, G_BUF};
use super::check_v2;
use crate::srv::timer;
use onyx_core::errno::KResult;
use onyx_core::formats::{OnyfsInode, ONYFS_BLOCK_SIZE, ONYFS_DIRECT_BLKS};

pub unsafe fn write(ino: u32, buf: *const u8, off: u32, len: u32) -> KResult<u32> {
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
    let mut written: u32 = 0;
    let mut cur_off = off;
    let mut remaining = len;
    while remaining > 0 {
        let blk_idx = (cur_off / ONYFS_BLOCK_SIZE as u32) as usize;
        let mut blk;
        if blk_idx < ONYFS_DIRECT_BLKS {
            blk = inode.blocks[blk_idx];
            if blk == 0 {
                blk = alloc_data_block()?;
                inode.blocks[blk_idx] = blk;
                let pb = &raw mut G_BUF;
                for b in (*pb).iter_mut() {
                    *b = 0;
                }
                journal_log(blk, &*pb)?;
                write_block(blk, &*pb)?;
            }
        } else {
            let ind_idx = blk_idx - ONYFS_DIRECT_BLKS;
            if ind_idx >= ONYFS_BLOCK_SIZE / 4 {
                break;
            }
            if inode.indirect == 0 {
                let ind_blk = alloc_data_block()?;
                inode.indirect = ind_blk;
                let pb = &raw mut G_BUF;
                for b in (*pb).iter_mut() {
                    *b = 0;
                }
                journal_log(ind_blk, &*pb)?;
                write_block(ind_blk, &*pb)?;
            }
            let pb = &raw mut G_BUF;
            read_block(inode.indirect, &mut *pb)?;
            let entry_off = ind_idx * 4;
            blk = u32::from_le_bytes([
                (*pb)[entry_off],
                (*pb)[entry_off + 1],
                (*pb)[entry_off + 2],
                (*pb)[entry_off + 3],
            ]);
            if blk == 0 {
                blk = alloc_data_block()?;
                let bytes = blk.to_le_bytes();
                (*pb)[entry_off] = bytes[0];
                (*pb)[entry_off + 1] = bytes[1];
                (*pb)[entry_off + 2] = bytes[2];
                (*pb)[entry_off + 3] = bytes[3];
                journal_log(inode.indirect, &*pb)?;
                write_block(inode.indirect, &*pb)?;
                for b in (*pb).iter_mut() {
                    *b = 0;
                }
                journal_log(blk, &*pb)?;
                write_block(blk, &*pb)?;
            }
        }
        let chunk_off = (cur_off % ONYFS_BLOCK_SIZE as u32) as usize;
        let chunk =
            (ONYFS_BLOCK_SIZE as u32 - cur_off % ONYFS_BLOCK_SIZE as u32).min(remaining) as usize;
        {
            let pb = &raw mut G_BUF;
            read_block(blk, &mut *pb)?;
            core::ptr::copy_nonoverlapping(
                buf.add(written as usize),
                (*pb).as_mut_ptr().add(chunk_off),
                chunk,
            );
            journal_log(blk, &*pb)?;
            write_block(blk, &*pb)?;
        }
        written += chunk as u32;
        cur_off += chunk as u32;
        remaining -= chunk as u32;
    }
    let end = off.wrapping_add(written);
    if (end as u64) > inode.size {
        inode.size = end as u64;
    }
    inode.mtime = *(&raw const timer::G_JIFFIES);
    write_inode(ino, &inode)?;
    journal_commit()?;
    Ok(written)
}

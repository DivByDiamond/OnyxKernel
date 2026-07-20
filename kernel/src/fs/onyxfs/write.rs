//! File write path — `write` (grow a file with new data) and `create` (new
//! regular file). v2-only. Each operation is wrapped in a journal transaction.
use super::alloc::{add_dirent, alloc_data_block, alloc_inode, free_data_block};
use super::inode::{read_inode, write_inode};
use super::journal::{journal_commit, journal_log};
use super::{G_BUF, G_VERSION, ONYFS_V1, read_block, write_block};
use crate::srv::timer;
use onyx_core::errno::{Errno, KResult};
use onyx_core::formats::{ONYFS_BLOCK_SIZE, ONYFS_DIRECT_BLKS, ONYFS_NAME_MAX, OnyfsInode};

/// Flush pending journal entries to disk. For OnyxFS this forces a journal
/// commit and a cache flush on the virtio device.
pub unsafe fn fsync(_ino: u32) -> KResult<()> {
    journal_commit()
}

/// Write data to a file at a given offset. Grows the file if needed.
/// Allocates new data blocks lazily for any block touched by the write that
/// is not yet mapped. Supports single-indirect blocks for files up to ~4 MB.
/// The inode's mtime and size are bumped as needed. The whole operation is
/// wrapped in a single journal transaction.
pub unsafe fn write(ino: u32, buf: *const u8, off: u32, len: u32) -> KResult<u32> {
    if *(&raw const G_VERSION) == ONYFS_V1 {
        return Err(Errno::NoSys);
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

/// Create a new regular file in a directory. Returns the new inode number.
/// The new inode is initialized with `mode`, size 0, no blocks, and current
/// timestamps. A dirent pointing to it is added to the parent directory's
/// first data block.
pub unsafe fn create(dir_ino: u32, name: &[u8], mode: u32) -> KResult<u32> {
    if *(&raw const G_VERSION) == ONYFS_V1 {
        return Err(Errno::NoSys);
    }
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
    add_dirent(dir_ino, name, new_ino, /*dtype=*/ 8)?;
    journal_commit()?;
    Ok(new_ino)
}

pub unsafe fn truncate(ino: u32) -> KResult<()> {
    if *(&raw const G_VERSION) == ONYFS_V1 {
        return Err(Errno::NoSys);
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

use super::alloc::{add_dirent, alloc_data_block, alloc_inode};
use super::inode::{read_inode, write_inode};
use super::journal::{journal_commit, journal_log};
use super::{G_BUF, G_VERSION, ONYFS_V1, read_block, write_block};
use crate::srv::timer;
use onyx_core::errno::{Errno, KResult};
use onyx_core::formats::{
    ONYFS_BLOCK_SIZE, ONYFS_DIRECT_BLKS, ONYFS_DT_LNK, ONYFS_NAME_MAX, OnyfsInode,
};

pub unsafe fn symlink(dir_ino: u32, name: &[u8], target: &[u8]) -> KResult<u32> {
    if *(&raw const G_VERSION) == ONYFS_V1 {
        return Err(Errno::NoSys);
    }
    if name.is_empty() || name.len() > ONYFS_NAME_MAX {
        return Err(Errno::Inval);
    }
    if target.is_empty() {
        return Err(Errno::Inval);
    }
    let new_ino = alloc_inode()?;
    let now = *(&raw const timer::G_JIFFIES);
    let data_blk = alloc_data_block()?;
    let mut inode = OnyfsInode {
        mode: ONYFS_DT_LNK,
        size: target.len() as u64,
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
    inode.blocks[0] = data_blk;
    {
        let pb = &raw mut G_BUF;
        for b in (*pb).iter_mut() {
            *b = 0;
        }
        for i in 0..target.len().min(ONYFS_BLOCK_SIZE) {
            (*pb)[i] = target[i];
        }
        journal_log(data_blk, &*pb)?;
        write_block(data_blk, &*pb)?;
    }
    write_inode(new_ino, &inode)?;
    add_dirent(dir_ino, name, new_ino, /*dtype=*/ 10)?;
    journal_commit()?;
    Ok(new_ino)
}

pub unsafe fn readlink(ino: u32, buf: *mut u8, len: u32) -> KResult<u32> {
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
    if inode.mode & 0o170000 != ONYFS_DT_LNK & 0o170000 {
        return Err(Errno::Inval);
    }
    let data_blk = inode.blocks[0];
    if data_blk == 0 {
        return Err(Errno::Io);
    }
    let target_len = inode.size.min(len as u64) as usize;
    {
        let pb = &raw mut G_BUF;
        read_block(data_blk, &mut *pb)?;
        for i in 0..target_len {
            *buf.add(i) = (*pb)[i];
        }
    }
    Ok(target_len as u32)
}

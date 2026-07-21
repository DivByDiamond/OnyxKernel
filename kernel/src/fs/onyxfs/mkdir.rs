//! `mkdir` — create a new directory with pre-populated "." and ".." entries.
//! v2-only. Wrapped in a single journal transaction.
use super::alloc::{add_dirent, alloc_data_block, alloc_inode};
use super::inode::write_inode;
use super::journal::{journal_commit, journal_log};
use super::{write_block, G_BUF, G_VERSION, ONYFS_V1};
use crate::srv::timer;
use onyx_core::errno::{Errno, KResult};
use onyx_core::formats::{
    OnyfsDirent, OnyfsInode, ONYFS_BLOCK_SIZE, ONYFS_DIRECT_BLKS, ONYFS_DT_DIR, ONYFS_NAME_MAX,
};

/// Create a new directory. Returns the new inode number. Like `create()` but
/// with `mode = ONYFS_DT_DIR`, and the new directory is given its own data
/// block pre-populated with the conventional "." and ".." entries.
pub unsafe fn mkdir(dir_ino: u32, name: &[u8]) -> KResult<u32> {
    if *(&raw const G_VERSION) == ONYFS_V1 {
        return Err(Errno::NoSys);
    }
    if name.is_empty() || name.len() > ONYFS_NAME_MAX {
        return Err(Errno::Inval);
    }
    let new_ino = alloc_inode()?;
    let now = *(&raw const timer::G_JIFFIES);
    let mut inode = OnyfsInode {
        mode: ONYFS_DT_DIR,
        size: 0,
        uid: 0,
        gid: 0,
        nlink: 2,
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
    // Allocate the new directory's own data block and seed it with "."/"..".
    let dir_blk = alloc_data_block()?;
    inode.blocks[0] = dir_blk;
    {
        let pb = &raw mut G_BUF;
        for b in (*pb).iter_mut() {
            *b = 0;
        }
        let mut dot_name = [0u8; ONYFS_NAME_MAX];
        dot_name[0] = b'.';
        let mut dotdot_name = [0u8; ONYFS_NAME_MAX];
        dotdot_name[0] = b'.';
        dotdot_name[1] = b'.';
        let dot = OnyfsDirent {
            name: dot_name,
            inode: new_ino,
            dtype: 4,
            name_len: 1,
            reserved: [0, 0],
        };
        let dotdot = OnyfsDirent {
            name: dotdot_name,
            inode: dir_ino,
            dtype: 4,
            name_len: 2,
            reserved: [0, 0],
        };
        let db1 = dot.to_bytes();
        let db2 = dotdot.to_bytes();
        for j in 0..OnyfsDirent::SIZE {
            (*pb)[j] = db1[j];
            (*pb)[OnyfsDirent::SIZE + j] = db2[j];
        }
        journal_log(dir_blk, &*pb)?;
        write_block(dir_blk, &*pb)?;
    }
    write_inode(new_ino, &inode)?;
    add_dirent(dir_ino, name, new_ino, /*dtype=*/ 4)?;
    journal_commit()?;
    Ok(new_ino)
}

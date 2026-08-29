use super::super::journal::journal_log;
use super::super::{G_BUF, G_SB, G_VERSION, ONYFS_V1, inodes_per_block, read_block, write_block};
use super::read::read_inode;
use crate::srv::timer;
use onyx_core::errno::{Errno, KResult};
use onyx_core::formats::{ONYFS_DIRECT_BLKS, OnyfsInode};

/// # Safety
///
/// Caller must not invoke onyxfs operations concurrently from multiple harts:
/// this uses the module-global G_BUF scratch block and reads G_SB/G_VERSION
/// (set by mount()). v1 filesystems are rejected. `ino` must be a valid inode
/// number; the slot offset is derived from the validated inode-table layout.
pub unsafe fn write_inode(ino: u32, inode: &OnyfsInode) -> KResult<()> {
    // SAFETY: single-threaded onyxfs exclusion (see # Safety); the slice
    // written into G_BUF is exactly OnyfsInode::SIZE at a slot offset that
    // fits because ONYFS_BLOCK_SIZE is a multiple of the inode size.
    unsafe {
        if G_VERSION == ONYFS_V1 {
            return Err(Errno::NoSys);
        }
        let ipb = inodes_per_block();
        let idx = (ino as usize).saturating_sub(1);
        let blk = (G_SB).inode_table_start + (idx / ipb) as u32;
        let slot = idx % ipb;
        {
            let pb = &raw mut G_BUF;
            read_block(blk, &mut *pb)
        }?;
        let bytes = inode.to_bytes();
        let off = slot * OnyfsInode::SIZE;
        let pb = &raw mut G_BUF;
        (&mut *pb)[off..(OnyfsInode::SIZE + off)].copy_from_slice(&bytes);
        journal_log(blk, &*pb)?;
        write_block(blk, &*pb)
    }
}

/// # Safety
///
/// Caller must not invoke onyxfs operations concurrently from multiple harts;
/// `ino` must be a valid inode number (validated inside read_inode).
pub unsafe fn update_mtime(ino: u32) -> KResult<()> {
    // SAFETY: single-threaded onyxfs exclusion (see # Safety); ino is
    // validated inside read_inode before any raw block access.
    unsafe {
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
        inode.mtime = timer::jiffies();
        write_inode(ino, &inode)
    }
}

/// # Safety
///
/// Caller must not invoke onyxfs operations concurrently from multiple harts;
/// `ino` must be a valid inode number (validated inside read_inode).
pub unsafe fn set_mode(ino: u32, mode: u32) -> KResult<()> {
    // SAFETY: single-threaded onyxfs exclusion (see # Safety); ino is
    // validated inside read_inode before any raw block access.
    unsafe {
        if G_VERSION == ONYFS_V1 {
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
        inode.mode = mode;
        inode.mtime = timer::jiffies();
        write_inode(ino, &inode)?;
        super::super::journal::journal_commit()
    }
}

/// # Safety
///
/// Caller must not invoke onyxfs operations concurrently from multiple harts;
/// `ino` must be a valid inode number (validated inside read_inode).
pub unsafe fn set_uid_gid(ino: u32, uid: u32, gid: u32) -> KResult<()> {
    // SAFETY: single-threaded onyxfs exclusion (see # Safety); ino is
    // validated inside read_inode before any raw block access.
    unsafe {
        if G_VERSION == ONYFS_V1 {
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
        inode.uid = uid;
        inode.gid = gid;
        inode.mtime = timer::jiffies();
        write_inode(ino, &inode)?;
        super::super::journal::journal_commit()
    }
}

/// # Safety
///
/// Caller must not invoke onyxfs operations concurrently from multiple harts;
/// `ino` must be a valid inode number (validated inside read_inode).
pub unsafe fn set_timestamps(ino: u32, mtime: u64, atime: u64) -> KResult<()> {
    // SAFETY: single-threaded onyxfs exclusion (see # Safety); ino is
    // validated inside read_inode before any raw block access.
    unsafe {
        if G_VERSION == ONYFS_V1 {
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
        inode.mtime = mtime;
        inode.atime = atime;
        write_inode(ino, &inode)
    }
}

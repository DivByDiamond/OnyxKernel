//! OnyxFS file creation: allocate an inode, register the dirent and
//! commit the journal.

use crate::fs::onyxfs::alloc::{add_dirent, alloc_inode};
use crate::fs::onyxfs::inode::write_inode;
use crate::fs::onyxfs::write::check_v2;
use crate::srv::timer;
use onyx_core::errno::{Errno, KResult};
use onyx_core::formats::{ONYFS_DIRECT_BLKS, ONYFS_NAME_MAX, OnyfsInode};

/// # Safety
///
/// Caller must not invoke onyxfs operations concurrently from multiple harts
/// (shared G_BUF scratch block, journal head, and dirent slots). `dir_ino`
/// must be a valid directory inode; `name` is length-checked in-line.
pub unsafe fn create(dir_ino: u32, name: &[u8], mode: u32) -> KResult<u32> {
    // SAFETY: single-threaded onyxfs exclusion (see # Safety); all on-disk
    // accesses go through the bounds-checked alloc/add/write helpers.
    unsafe {
        check_v2()?;
        if name.is_empty() || name.len() > ONYFS_NAME_MAX {
            return Err(Errno::Inval);
        }
        let new_ino = alloc_inode()?;
        let now = timer::jiffies();
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
        crate::fs::onyxfs::journal::journal_commit()?;
        Ok(new_ino)
    }
}

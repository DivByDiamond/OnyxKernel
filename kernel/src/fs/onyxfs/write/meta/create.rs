//! OnyxFS file creation: allocate an inode, register the dirent and
//! commit the journal.

use super::truncate::truncate;
use crate::fs::onyxfs::OnyfsStat;
use crate::fs::onyxfs::alloc::{add_dirent, alloc_inode};
use crate::fs::onyxfs::inode::write_inode;
use crate::fs::onyxfs::lookup::lookup_in;
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
        crate::fs::onyxfs::journal::transaction(|| {
            check_v2()?;
            if name.is_empty() || name.len() > ONYFS_NAME_MAX {
                return Err(Errno::Inval);
            }
            // An existing regular file is truncated and reused, as creat(2) does.
            // Allocating a fresh inode would let add_dirent replace the old
            // dirent and orphan the old inode together with all of its data
            // blocks, which exhausts the volume when a large file is rewritten.
            let mut st = OnyfsStat::default();
            if let Ok(old_ino) = lookup_in(dir_ino, name, &mut st) {
                match st.mode & 0o170000 {
                    0o040000 => return Err(Errno::IsDir),
                    0o120000 => {}
                    _ => {
                        truncate(old_ino)?;
                        return Ok(old_ino);
                    }
                }
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
        })
    }
}

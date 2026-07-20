//! Stateful readdir — `readdir_entry` returns one directory entry per call
//! (the VFS layer maintains the cursor). Uses `parse_dirent` from `lookup.rs`.
use super::inode::read_inode;
use super::lookup::parse_dirent;
use super::{G_BUF, dirents_per_block, read_block};
use onyx_core::errno::{Errno, KResult};
use onyx_core::formats::{ONYFS_DIRECT_BLKS, ONYFS_DT_DIR, ONYFS_NAME_MAX, OnyfsInode};

/// Read a directory entry by index. Returns (inode, name_len, is_dir).
/// Used by SYS_readdir and getdents64.
/// Scans across all direct blocks, skipping zero-inode (deleted/unused) entries.
pub unsafe fn readdir_entry(
    dir_ino: u32,
    entry_idx: u32,
    name_out: *mut u8,
    name_len: usize,
) -> KResult<Option<u32>> {
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
    read_inode(dir_ino, &mut inode)?;
    if inode.mode & 0o170000 != ONYFS_DT_DIR & 0o170000 {
        return Err(Errno::NotDir);
    }
    let dpb = dirents_per_block();
    let max_entries = dpb * ONYFS_DIRECT_BLKS;
    let mut last_block_idx = usize::MAX;

    for abs_idx in entry_idx as usize..max_entries {
        let block_idx = abs_idx / dpb;
        let slot_idx = abs_idx % dpb;

        if block_idx >= ONYFS_DIRECT_BLKS {
            return Ok(None);
        }

        let dir_blk = inode.blocks[block_idx];
        if dir_blk == 0 {
            return Ok(None);
        }

        if block_idx != last_block_idx {
            let pb = &raw mut G_BUF;
            read_block(dir_blk, &mut *pb)?;
            last_block_idx = block_idx;
        }

        let d = parse_dirent(slot_idx)?;

        if d.inode == 0 {
            continue;
        }

        let nl = if d.name_len > 0 && (d.name_len as usize) <= ONYFS_NAME_MAX {
            d.name_len as usize
        } else {
            d.name
                .iter()
                .position(|&b| b == 0)
                .unwrap_or(ONYFS_NAME_MAX)
        };
        let copy_n = nl.min(name_len.saturating_sub(1));
        for i in 0..copy_n {
            *name_out.add(i) = d.name[i];
        }
        if copy_n < name_len {
            *name_out.add(copy_n) = 0;
        }
        return Ok(Some(d.inode));
    }

    Ok(None)
}

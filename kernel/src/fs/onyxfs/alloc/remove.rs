use super::super::inode;
use super::super::journal::journal_log;
use super::super::{
    G_BUF, G_VERSION, ONYFS_V1, ONYFS_V1_DIRENT_SIZE, dirents_per_block, read_block, write_block,
};
use onyx_core::errno::{Errno, KResult};
use onyx_core::formats::{
    ONYFS_BLOCK_SIZE, ONYFS_DIRECT_BLKS, ONYFS_NAME_MAX, OnyfsDirent, OnyfsInode,
};

/// Remove a dirent from a directory inode.
///
/// Bug #19 fix: previously this only scanned `dir_inode.blocks[0]`, so
/// entries that lived in blocks[1..9] could never be removed (returned
/// NoEnt). We now scan every direct block.
pub unsafe fn remove_dirent(dir_ino: u32, name: &[u8]) -> KResult<()> {
    let mut dir_inode = OnyfsInode {
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
    inode::read_inode(dir_ino, &mut dir_inode)?;
    let dpb = dirents_per_block();
    let entry_size = match *(&raw const G_VERSION) {
        ONYFS_V1 => ONYFS_V1_DIRENT_SIZE,
        _ => OnyfsDirent::SIZE,
    };

    for blk_idx in 0..ONYFS_DIRECT_BLKS {
        let dir_blk = dir_inode.blocks[blk_idx];
        if dir_blk == 0 {
            continue;
        }
        let pb = &raw mut G_BUF;
        read_block(dir_blk, &mut *pb)?;

        for i in 0..dpb {
            let off = i * entry_size;
            if off + entry_size > ONYFS_BLOCK_SIZE {
                break;
            }
            let inode_off = off + 32;
            let existing = u32::from_le_bytes([
                (*pb)[inode_off],
                (*pb)[inode_off + 1],
                (*pb)[inode_off + 2],
                (*pb)[inode_off + 3],
            ]);
            if existing == 0 {
                continue;
            }
            let existing_name = &(&*pb)[off..off + ONYFS_NAME_MAX];
            let mut match_len = 0;
            while match_len < name.len() && match_len < ONYFS_NAME_MAX {
                if existing_name[match_len] != name[match_len] {
                    break;
                }
                match_len += 1;
            }
            if match_len == name.len()
                && (match_len >= ONYFS_NAME_MAX || existing_name[match_len] == 0)
            {
                // Zero the dirent's inode field — this marks the slot free.
                // The name bytes are left in place; future add_dirent() will
                // overwrite them when reusing the slot.
                (*pb)[inode_off] = 0;
                (*pb)[inode_off + 1] = 0;
                (*pb)[inode_off + 2] = 0;
                (*pb)[inode_off + 3] = 0;
                journal_log(dir_blk, &*pb)?;
                write_block(dir_blk, &*pb)?;
                return Ok(());
            }
        }
    }
    Err(Errno::NoEnt)
}

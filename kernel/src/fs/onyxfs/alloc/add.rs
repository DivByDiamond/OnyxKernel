use super::super::inode;
use super::super::journal::journal_log;
use super::super::{
    G_BUF, G_SB, G_VERSION, ONYFS_V1, ONYFS_V1_DIRENT_SIZE, dirents_per_block, read_block,
    write_block,
};
use super::bitmap::alloc_data_block;
use onyx_core::errno::{Errno, KResult};
use onyx_core::formats::{
    ONYFS_BLOCK_SIZE, ONYFS_DIRECT_BLKS, ONYFS_NAME_MAX, OnyfsDirent, OnyfsInode,
};

/// Add (or update) a dirent in a directory inode.
///
/// Bug #19 fix: previously this only looked at `dir_inode.blocks[0]`, so
/// adding the 103rd entry (one full v2 block holds 102 dirents) returned
/// NoSpace even when blocks[1..9] were unused. We now:
///   1. First pass: scan every direct block for an existing entry with
///      the same name and update it in place.
///   2. Second pass: scan every direct block for a free slot (inode==0)
///      and write the new entry there.
///   3. If no free slot exists in any existing block, allocate a fresh
///      direct block in the first empty `blocks[i]` slot, zero it, and
///      write the new entry as its first slot.
pub unsafe fn add_dirent(dir_ino: u32, name: &[u8], target_ino: u32, dtype: u8) -> KResult<()> {
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

    // Helper: write the dirent bytes for `name`/`target_ino`/`dtype` at
    // slot `i` in G_BUF (already loaded with the block contents).
    macro_rules! write_slot {
        ($pb:expr, $i:expr) => {{
            let off = $i * entry_size;
            let inode_off = off + 32;
            let ino_bytes = target_ino.to_le_bytes();
            ($pb)[inode_off] = ino_bytes[0];
            ($pb)[inode_off + 1] = ino_bytes[1];
            ($pb)[inode_off + 2] = ino_bytes[2];
            ($pb)[inode_off + 3] = ino_bytes[3];
            // Write the name field (zero-padded to ONYFS_NAME_MAX).
            let n = name.len().min(ONYFS_NAME_MAX);
            for j in 0..n {
                ($pb)[off + j] = name[j];
            }
            for j in n..ONYFS_NAME_MAX {
                ($pb)[off + j] = 0;
            }
            if *(&raw const G_VERSION) != ONYFS_V1 {
                ($pb)[off + 36] = dtype;
                ($pb)[off + 37] = n as u8;
            }
        }};
    }

    // Pass 1: look for an existing entry with the same name to overwrite.
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
                write_slot!(&mut *pb, i);
                journal_log(dir_blk, &*pb)?;
                write_block(dir_blk, &*pb)?;
                return Ok(());
            }
        }
    }

    // Pass 2: look for a free slot (inode == 0) in any existing block.
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
            if existing != 0 {
                continue;
            }
            write_slot!(&mut *pb, i);
            journal_log(dir_blk, &*pb)?;
            write_block(dir_blk, &*pb)?;
            return Ok(());
        }
    }

    // Pass 3: allocate a new direct block in the first empty slot.
    for blk_idx in 0..ONYFS_DIRECT_BLKS {
        if dir_inode.blocks[blk_idx] != 0 {
            continue;
        }
        let new_blk = alloc_data_block()?;
        dir_inode.blocks[blk_idx] = new_blk;
        let pb = &raw mut G_BUF;
        // Zero the new block on disk first.
        for b in (*pb).iter_mut() {
            *b = 0;
        }
        journal_log(new_blk, &*pb)?;
        write_block(new_blk, &*pb)?;
        // Write the dirent as the first slot.
        write_slot!(&mut *pb, 0);
        journal_log(new_blk, &*pb)?;
        write_block(new_blk, &*pb)?;
        // Persist the updated inode (now references the new block).
        inode::write_inode(dir_ino, &dir_inode)?;
        return Ok(());
    }

    // All direct blocks are full and there's no room for a new one —
    // would need to spill into the indirect block. Not implemented yet.
    Err(Errno::NoSpace)
}

//! File read path — `read` from a regular file inode.
use super::inode::read_inode;
use super::{G_BUF, G_VERSION, ONYFS_V1, read_block};
use onyx_core::errno::{Errno, KResult};
use onyx_core::formats::{ONYFS_BLOCK_SIZE, ONYFS_DIRECT_BLKS, OnyfsInode};

pub unsafe fn read(ino: u32, buf: *mut u8, off: u32, len: u32) -> KResult<u32> {
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
    let _ = ONYFS_V1; // suppress unused import warning if v1 path not hit
    read_inode(ino, &mut inode)?;
    // inode.size is u64 in v2; cap to u32 for the VFS-facing API.
    let file_size = inode.size.min(u32::MAX as u64) as u32;
    let mut read_total: u32 = 0;
    let mut off = off;
    let mut remaining = len.min(file_size.saturating_sub(off));

    let bs = ONYFS_BLOCK_SIZE as u32;
    // Bug #17 fix: start from the block that contains `off`, not from
    // blocks[0]. The previous code unconditionally iterated from
    // inode.blocks[0], so any non-zero offset returned data from the
    // start of the file.
    let mut abs_blk_idx: u32 = off / bs;

    // Capacity of the single-indirect block in u32 entries.
    let indirect_capacity = (ONYFS_BLOCK_SIZE / 4) as u32;

    while remaining > 0 {
        // Resolve the physical block number for abs_blk_idx, traversing
        // the single-indirect block when abs_blk_idx >= ONYFS_DIRECT_BLKS.
        // Bug #18 fix: previously only the 10 direct blocks were readable,
        // so files larger than ~40 KB returned short reads / NoEnt.
        //
        // We use a stack-local buffer for indirect-block reads so we don't
        // clobber G_BUF (which is used by read_block inside the data path).
        // Double-indirect traversal is intentionally not implemented here
        // — write.rs only supports single-indirect, so files never grow
        // past the single-indirect capacity on this filesystem anyway.
        let blk = if abs_blk_idx < ONYFS_DIRECT_BLKS as u32 {
            inode.blocks[abs_blk_idx as usize]
        } else {
            let ind_idx = abs_blk_idx - ONYFS_DIRECT_BLKS as u32;
            if inode.indirect == 0 || ind_idx >= indirect_capacity {
                break;
            }
            let mut ind_buf = [0u8; ONYFS_BLOCK_SIZE];
            read_block(inode.indirect, &mut ind_buf)?;
            let entry_off = (ind_idx as usize) * 4;
            u32::from_le_bytes([
                ind_buf[entry_off],
                ind_buf[entry_off + 1],
                ind_buf[entry_off + 2],
                ind_buf[entry_off + 3],
            ])
        };

        if blk == 0 {
            break;
        }

        {
            let pb = &raw mut G_BUF;
            read_block(blk, &mut *pb)
        }?;
        let chunk_off = (off % bs) as usize;
        let chunk = (bs - off % bs).min(remaining) as usize;
        core::ptr::copy_nonoverlapping(
            (*(&raw const G_BUF)).as_ptr().add(chunk_off),
            buf.add(read_total as usize),
            chunk,
        );
        read_total += chunk as u32;
        off += chunk as u32;
        remaining -= chunk as u32;
        abs_blk_idx += 1;
    }
    Ok(read_total)
}

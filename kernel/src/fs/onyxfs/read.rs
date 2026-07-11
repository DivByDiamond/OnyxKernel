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
    for &blk in inode.blocks.iter() {
        if remaining == 0 || blk == 0 {
            break;
        }
        {
            let pb = &raw mut G_BUF;
            read_block(blk, &mut *pb)
        }?;
        let chunk_off = (off % ONYFS_BLOCK_SIZE as u32) as usize;
        let chunk =
            (ONYFS_BLOCK_SIZE as u32 - off % ONYFS_BLOCK_SIZE as u32).min(remaining) as usize;
        core::ptr::copy_nonoverlapping(
            (*(&raw const G_BUF)).as_ptr().add(chunk_off),
            buf.add(read_total as usize),
            chunk,
        );
        read_total += chunk as u32;
        off += chunk as u32;
        remaining -= chunk as u32;
    }
    Ok(read_total)
}

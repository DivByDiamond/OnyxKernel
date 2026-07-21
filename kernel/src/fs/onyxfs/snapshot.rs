//! Snapshot management — create (RLE-compressed COW). Rollback and list
//! operations live in `snapshot_io.rs`.
//!
//! Layout in the snapshot area (starting at `super.snapshot_area_start`):
//!   block 0: array of `SnapshotMeta` records (64 bytes each)
//!   block 1 + (id-1)*SNAPSHOT_BLOCKS_EACH .. : per-snapshot data
//!     = inode-table copy + data-bitmap copy
//!
//! Per-snapshot data occupies `SNAPSHOT_BLOCKS_EACH` (64) consecutive blocks.
//! The first block is a header describing the compressed slots; the remaining
//! 63 blocks hold compressed block data, with each compressed block occupying
//! exactly 2 on-disk blocks (8192 bytes, enough for any 4096-byte input even
//! in the worst-case RLE expansion).
use super::compress::rle_compress;
use super::{
    inode_table_block_count, inodes_per_block, persist_superblock, read_block, write_block, G_BUF,
    G_SB, SNAPSHOT_BLOCKS_EACH,
};
use crate::srv::timer;
use onyx_core::errno::{Errno, KResult};
use onyx_core::formats::{OnyfsInode, SnapshotMeta, ONYFS_BLOCK_SIZE, ONYFS_FEAT_SNAPSHOTS};

pub(super) const SNAPSHOT_SLOTS: u32 = 31;
pub(super) const SNAPSHOT_SLOT_BLKS: u32 = 2;

/// Create a snapshot: walk the inode table to enumerate all live blocks
/// (inode-table + data-bitmap + used data blocks), RLE-compress each block,
/// and store the compressed data in the snapshot area. Also writes a
/// `SnapshotMeta` record and bumps `snapshot_count`. Returns the new ID.
pub unsafe fn snapshot_create(name: &[u8]) -> KResult<u32> {
    let sb_ptr = &raw const G_SB;
    if (*sb_ptr).snapshot_area_start == 0 || (*sb_ptr).feature_flags & ONYFS_FEAT_SNAPSHOTS == 0 {
        return Err(Errno::NoSys);
    }
    let new_id = (*sb_ptr).snapshot_count + 1;
    let snap_data_start = (*sb_ptr).snapshot_area_start + 1 + (new_id - 1) * SNAPSHOT_BLOCKS_EACH;

    // Enumerate the live blocks to snapshot: (block_num, comp_size=0).
    let mut blocks: [(u32, u32); SNAPSHOT_SLOTS as usize] = [(0, 0); SNAPSHOT_SLOTS as usize];
    let mut n_blocks: usize = 0;
    macro_rules! push_block {
        ($b:expr) => {{
            if n_blocks >= SNAPSHOT_SLOTS as usize {
                return Err(Errno::NoMem);
            }
            let mut dup = false;
            for j in 0..n_blocks {
                if blocks[j].0 == $b {
                    dup = true;
                    break;
                }
            }
            if !dup {
                blocks[n_blocks] = ($b, 0);
                n_blocks += 1;
            }
        }};
    }
    let inode_tbl_blocks = inode_table_block_count();
    for i in 0..inode_tbl_blocks {
        push_block!((*sb_ptr).inode_table_start + i);
    }
    push_block!((*sb_ptr).data_bitmap_start);
    for blk_idx in 0..inode_tbl_blocks {
        let pb = &raw mut G_BUF;
        read_block((*sb_ptr).inode_table_start + blk_idx, &mut *pb)?;
        let ipb = inodes_per_block();
        for slot in 0..ipb {
            let off = slot * OnyfsInode::SIZE;
            if off + OnyfsInode::SIZE > ONYFS_BLOCK_SIZE {
                break;
            }
            let buf_view: &[u8] = &*pb;
            let inode = match OnyfsInode::from_bytes(&buf_view[off..off + OnyfsInode::SIZE]) {
                Some(i) => i,
                None => continue,
            };
            if inode.mode == 0 {
                continue;
            }
            for &b in inode.blocks.iter() {
                if b != 0 {
                    push_block!(b);
                }
            }
        }
    }

    // Compress and store each block in its 2-block slot.
    let mut comp_buf = [0u8; 8192];
    let mut blk_buf = [0u8; ONYFS_BLOCK_SIZE];
    for i in 0..n_blocks {
        let block_num = blocks[i].0;
        read_block(block_num, &mut blk_buf)?;
        let comp_size = rle_compress(&blk_buf, &mut comp_buf);
        let stored_size: u32 = if comp_size == 0 || comp_size > 8192 {
            comp_buf[..ONYFS_BLOCK_SIZE].copy_from_slice(&blk_buf);
            ONYFS_BLOCK_SIZE as u32
        } else {
            comp_size as u32
        };
        let slot_start = snap_data_start + 1 + (i as u32) * SNAPSHOT_SLOT_BLKS;
        let mut out_blk = [0u8; ONYFS_BLOCK_SIZE];
        out_blk.copy_from_slice(&comp_buf[..ONYFS_BLOCK_SIZE]);
        write_block(slot_start, &out_blk)?;
        out_blk.copy_from_slice(&comp_buf[ONYFS_BLOCK_SIZE..8192]);
        write_block(slot_start + 1, &out_blk)?;
        blocks[i].1 = stored_size;
    }

    // Write header block: n_entries + (block_num, comp_size) pairs.
    let mut header = [0u8; ONYFS_BLOCK_SIZE];
    header[0..4].copy_from_slice(&(n_blocks as u32).to_le_bytes());
    for i in 0..n_blocks {
        let off = 4 + i * 8;
        header[off..off + 4].copy_from_slice(&blocks[i].0.to_le_bytes());
        header[off + 4..off + 8].copy_from_slice(&blocks[i].1.to_le_bytes());
    }
    write_block(snap_data_start, &header)?;

    // Write SnapshotMeta into the area header block.
    let mut name_buf = [0u8; 32];
    let n = name.len().min(32);
    name_buf[..n].copy_from_slice(&name[..n]);
    let meta = SnapshotMeta {
        id: new_id,
        timestamp: *(&raw const timer::G_JIFFIES),
        root_inode_snapshot: (*sb_ptr).root_inode,
        block_count: n_blocks as u32,
        name: name_buf,
        parent_id: 0,
        flags: 0,
        reserved: [0; 4],
    };
    let pb = &raw mut G_BUF;
    read_block((*sb_ptr).snapshot_area_start, &mut *pb)?;
    let meta_off = ((new_id - 1) as usize) * SnapshotMeta::SIZE;
    if meta_off + SnapshotMeta::SIZE > ONYFS_BLOCK_SIZE {
        return Err(Errno::NoMem);
    }
    let meta_bytes = meta.to_bytes();
    for i in 0..SnapshotMeta::SIZE {
        (*pb)[meta_off + i] = meta_bytes[i];
    }
    write_block((*sb_ptr).snapshot_area_start, &*pb)?;

    (*(&raw mut G_SB)).snapshot_count = new_id;
    persist_superblock()?;
    Ok(new_id)
}

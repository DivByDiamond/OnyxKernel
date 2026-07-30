//! Snapshot rollback and list operations.
//!
//! `snapshot_rollback` reads the per-snapshot header, RLE-decompresses each
//! stored block (or copies it raw if it was stored uncompressed), and writes
//! the result back to its original block number. This restores inode table,
//! data bitmap, and all live data blocks captured at snapshot time — a true
//! COW rollback.
//!
//! `snapshot_list` writes each snapshot name (NUL-terminated, newline-
//! separated) into `names_out` and returns the number listed.
use super::compress::rle_decompress;
use super::journal::{journal_commit, journal_log};
use super::{
    G_BUF, G_SB, SNAPSHOT_BLOCKS_EACH, SNAPSHOT_SLOT_BLKS, SNAPSHOT_SLOTS, read_block, write_block,
};
use onyx_core::errno::{Errno, KResult};
use onyx_core::formats::{ONYFS_BLOCK_SIZE, SnapshotMeta};

/// Restore filesystem state from a snapshot.
///
/// Bug #10 (fs critical list, item 11) fix: previously each block write
/// went directly to disk via `write_block` with no journaling, so a crash
/// mid-rollback left the filesystem in an inconsistent state (some blocks
/// restored, others not). Now every block write is preceded by a
/// `journal_log` call and the whole rollback is wrapped in a single
/// transaction via `journal_commit` at the end. If a crash occurs mid-
/// rollback, the next mount's `journal_recover` will replay the remaining
/// writes and bring the filesystem to a consistent snapshot-restored state.
pub unsafe fn snapshot_rollback(snapshot_id: u32) -> KResult<()> {
    let sb_ptr = &raw const G_SB;
    if (*sb_ptr).snapshot_area_start == 0 {
        return Err(Errno::NoSys);
    }
    if snapshot_id == 0 || snapshot_id > (*sb_ptr).snapshot_count {
        return Err(Errno::NoEnt);
    }
    let snap_data_start =
        (*sb_ptr).snapshot_area_start + 1 + (snapshot_id - 1) * SNAPSHOT_BLOCKS_EACH;

    let mut header = [0u8; ONYFS_BLOCK_SIZE];
    read_block(snap_data_start, &mut header)?;
    let n_blocks = u32::from_le_bytes([header[0], header[1], header[2], header[3]]) as usize;
    if n_blocks > SNAPSHOT_SLOTS as usize {
        return Err(Errno::Io);
    }

    let mut comp_buf = [0u8; 8192];
    let mut blk_buf = [0u8; ONYFS_BLOCK_SIZE];
    for i in 0..n_blocks {
        let off = 4 + i * 8;
        let block_num = u32::from_le_bytes([
            header[off],
            header[off + 1],
            header[off + 2],
            header[off + 3],
        ]);
        let comp_size = u32::from_le_bytes([
            header[off + 4],
            header[off + 5],
            header[off + 6],
            header[off + 7],
        ]) as usize;

        // Read 2 blocks of compressed data.
        let slot_start = snap_data_start + 1 + (i as u32) * SNAPSHOT_SLOT_BLKS;
        read_block(slot_start, &mut blk_buf)?;
        comp_buf[..ONYFS_BLOCK_SIZE].copy_from_slice(&blk_buf);
        read_block(slot_start + 1, &mut blk_buf)?;
        comp_buf[ONYFS_BLOCK_SIZE..8192].copy_from_slice(&blk_buf);

        let mut out_buf = [0u8; ONYFS_BLOCK_SIZE];
        if comp_size == ONYFS_BLOCK_SIZE {
            // Stored raw.
            out_buf.copy_from_slice(&comp_buf[..ONYFS_BLOCK_SIZE]);
        } else {
            let dec = rle_decompress(&comp_buf[..comp_size], &mut out_buf);
            if dec != ONYFS_BLOCK_SIZE {
                return Err(Errno::Io);
            }
        }
        // Bug #10 fix: journal the write so a crash mid-rollback can be
        // recovered on next mount. Without this, a partial rollback would
        // leave the filesystem with a mix of pre-rollback and post-rollback
        // blocks — silently corrupting it.
        journal_log(block_num, &out_buf)?;
        write_block(block_num, &out_buf)?;
    }
    // Commit the rollback transaction. After this point the rollback is
    // durable: either all blocks are restored or, if a crash happened
    // before commit, the next mount's journal_recover will replay the
    // remaining writes.
    journal_commit()?;
    Ok(())
}

/// List all snapshots: write each snapshot name (NUL-terminated, newline-
/// separated) into `names_out`. Returns the number of snapshots listed.
pub unsafe fn snapshot_list(names_out: *mut u8, max_len: usize) -> KResult<u32> {
    let sb_ptr = &raw const G_SB;
    if (*sb_ptr).snapshot_area_start == 0 {
        return Ok(0);
    }
    let count = (*sb_ptr).snapshot_count;
    if count == 0 || max_len == 0 {
        return Ok(0);
    }
    let pb = &raw mut G_BUF;
    read_block((*sb_ptr).snapshot_area_start, &mut *pb)?;
    let mut written: usize = 0;
    let mut listed: u32 = 0;
    for i in 0..count {
        let off = (i as usize) * SnapshotMeta::SIZE;
        if off + SnapshotMeta::SIZE > ONYFS_BLOCK_SIZE {
            break;
        }
        let buf_view: &[u8] = &*pb;
        let slice = &buf_view[off..off + SnapshotMeta::SIZE];
        let meta = match SnapshotMeta::from_bytes(slice) {
            Some(m) => m,
            None => continue,
        };
        // Copy name (up to 32 bytes, stopping at NUL) + trailing newline.
        let mut name_len = 0;
        for j in 0..32 {
            if meta.name[j] == 0 {
                break;
            }
            name_len += 1;
        }
        for j in 0..name_len {
            if written + 1 >= max_len {
                return Ok(listed); // out of space
            }
            *names_out.add(written) = meta.name[j];
            written += 1;
        }
        if written + 1 < max_len {
            *names_out.add(written) = b'\n';
            written += 1;
        }
        listed += 1;
    }
    if written < max_len {
        *names_out.add(written) = 0;
    }
    Ok(listed)
}

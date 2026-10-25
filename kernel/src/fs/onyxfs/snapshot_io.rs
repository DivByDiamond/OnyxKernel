//! Public rollback/list entry points under root VFS exclusion.
use super::snapshot::Disk;
use super::snapshot_engine;
use super::{G_BUF, G_SB, read_block};
use onyx_core::errno::KResult;
use onyx_core::formats::{ONYFS_BLOCK_SIZE, SnapshotMeta};

/// Restore a snapshot, retaining a durable pending marker until every full
/// target block has been restored. Mount completes interrupted restores.
/// # Safety
/// Caller holds root filesystem exclusion and no other disk I/O can interleave.
pub unsafe fn snapshot_rollback(id: u32) -> KResult<()> {
    // SAFETY: caller owns the root mount lock across commit and restore.
    unsafe {
        super::snapshot_access::access(super::G_SNAPSHOT_QUARANTINED)?;
        super::journal::journal_commit()?;
        let mut quarantine = false;
        let result = snapshot_engine::rollback(&mut Disk, &mut G_SB, id, &mut quarantine);
        super::G_SNAPSHOT_QUARANTINED = quarantine;
        result
    }
}

/// List all snapshots: write each snapshot name (NUL-terminated, newline-
/// separated) into `names_out`. Returns the number of snapshots listed.
///
/// # Safety
///
/// `names_out` must be writable for `max_len` bytes and stay valid for the
/// call (the syscall layer translates user buffers before reaching fs/);
/// every write below is guarded by a `written + 1 < max_len` check. Caller
/// must not invoke onyxfs operations concurrently from multiple harts (the
/// G_BUF scratch global is unsynchronized).
pub unsafe fn snapshot_list(names_out: *mut u8, max_len: usize) -> KResult<u32> {
    // SAFETY: see # Safety for names_out validity; all writes are bounded by
    // the max_len checks, and G_BUF slices are bounds-checked against the
    // block size and SnapshotMeta::SIZE.
    unsafe {
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
}

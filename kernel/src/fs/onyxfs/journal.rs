//! Ordered metadata undo journal with full-block before-images.
//!
//! Each distinct metadata block is saved once before its first home write.
//! The ONJU header publishes the target and checksum only after its undo
//! image completes. Commit clears just this header after all home writes.
//! Recovery rolls back a nonempty header; it is restartable after I/O errors.
//! Regular file data is ordered before metadata commit but is not atomic.
//! Legacy redo recovery remains supported. See docs/ONYXFS_JOURNAL.md.
use super::{G_JOURNAL_HEAD, G_SB, G_SNAPSHOT_QUARANTINED, raw_read_block, raw_write_block};
use onyx_core::errno::{Errno, KResult};
use onyx_core::formats::{
    ONYFS_BLOCK_SIZE, ONYFS_JOURNAL_BLOCK_WRITE, ONYFS_JOURNAL_COMMIT_END,
    ONYFS_JOURNAL_COMMIT_START, OnyfsSuper,
};

mod format;
mod legacy;
mod transaction;
use format::{MAGIC, MAX_ENTRIES, checksum, get, put, seal, valid_target, validate};
pub(crate) use transaction::transaction;

/// # Safety
/// Caller holds FS_LOCK (SIE=0) or single-threaded boot exclusion. `sb`
/// describes a validated journal. Home writes must already have completed.
unsafe fn clear(sb: &OnyfsSuper) -> KResult<()> {
    // SAFETY: the header occupies one validated journal block.
    unsafe {
        let mut header = [0; ONYFS_BLOCK_SIZE];
        seal(&mut header);
        raw_write_block(sb.journal_start, &header)?;
        G_JOURNAL_HEAD = 0;
        Ok(())
    }
}

/// Save the full original block before changing metadata. Repeated writes
/// share one undo slot; capacity exhaustion fails instead of auto-committing.
/// # Safety
/// Caller holds FS_LOCK (SIE=0) or boot exclusion across log and home write.
pub(super) unsafe fn journal_before_write(block: u32) -> KResult<()> {
    // SAFETY: caller owns the mounted volume across the complete operation.
    unsafe {
        let result = before_write(block);
        if result.is_err() {
            G_SNAPSHOT_QUARANTINED = true;
        }
        result
    }
}

/// # Safety
/// Same exclusive volume contract as journal_before_write.
#[inline(never)]
unsafe fn before_write(block: u32) -> KResult<()> {
    // SAFETY: serialized globals and raw block I/O remain within the volume.
    unsafe {
        super::snapshot::access::access(G_SNAPSHOT_QUARANTINED)?;
        let sb = G_SB;
        if sb.journal_start == 0 || sb.journal_size == 0 {
            return Ok(());
        }
        valid_target(block, &sb)?;
        let mut header = [0; ONYFS_BLOCK_SIZE];
        raw_read_block(sb.journal_start, &mut header)?;
        if get(&header, 0) != MAGIC {
            journal_recover()?;
            raw_read_block(sb.journal_start, &mut header)?;
        }
        let count = validate(&header, &sb)?;
        if (0..count).any(|i| get(&header, 16 + i * 8) == block) {
            return Ok(());
        }
        if count >= MAX_ENTRIES || count + 1 >= sb.journal_size as usize {
            return Err(Errno::NoSpace);
        }
        let mut original = [0; ONYFS_BLOCK_SIZE];
        raw_read_block(block, &mut original)?;
        raw_write_block(sb.journal_start + 1 + count as u32, &original)?;
        put(&mut header, 16 + count * 8, block);
        put(&mut header, 20 + count * 8, checksum(&original));
        put(&mut header, 8, (count + 1) as u32);
        seal(&mut header);
        raw_write_block(sb.journal_start, &header)?;
        G_JOURNAL_HEAD = (count + 1) as u32;
        Ok(())
    }
}

/// Complete an operation. VFS transactions defer this to their outer edge.
/// # Safety
/// Caller owns the mounted volume under FS_LOCK or boot exclusion.
pub(super) unsafe fn journal_commit() -> KResult<()> {
    // SAFETY: the operation boundary owns all journal and home writes.
    unsafe {
        if transaction::deferred() {
            return Ok(());
        }
        super::snapshot::access::access(G_SNAPSHOT_QUARANTINED)?;
        let sb = G_SB;
        if sb.journal_start == 0 || sb.journal_size == 0 {
            return Ok(());
        }
        if G_JOURNAL_HEAD != 0
            && let Err(error) = clear(&sb)
        {
            G_SNAPSHOT_QUARANTINED = true;
            return Err(error);
        }
        Ok(())
    }
}

/// Restore unfinished metadata updates, retaining the header until every
/// full before-image is restored. Failed recovery keeps the volume gated.
/// # Safety
/// Caller owns FS_LOCK or boot exclusion; no normal I/O may interleave.
pub unsafe fn journal_recover() -> KResult<()> {
    // SAFETY: raw I/O is permitted only during exclusive recovery.
    unsafe {
        let result = recover();
        if result.is_err() {
            G_SNAPSHOT_QUARANTINED = true;
        }
        result
    }
}

/// # Safety
/// Same exclusion and validated volume contract as journal_recover.
#[inline(never)]
unsafe fn recover() -> KResult<()> {
    // SAFETY: header/targets are validated before any home block is changed.
    unsafe {
        let sb = G_SB;
        if sb.journal_start == 0 || sb.journal_size == 0 {
            return Ok(());
        }
        valid_target(0, &sb)?;
        let mut header = [0; ONYFS_BLOCK_SIZE];
        raw_read_block(sb.journal_start, &mut header)?;
        match get(&header, 0) {
            MAGIC => {}
            ONYFS_JOURNAL_COMMIT_START | ONYFS_JOURNAL_BLOCK_WRITE | ONYFS_JOURNAL_COMMIT_END => {
                return legacy::recover(&sb);
            }
            _ => return Err(Errno::Io),
        }
        let count = validate(&header, &sb)?;
        let mut original = [0; ONYFS_BLOCK_SIZE];
        for i in 0..count {
            raw_read_block(sb.journal_start + 1 + i as u32, &mut original)?;
            if checksum(&original) != get(&header, 20 + i * 8) {
                return Err(Errno::Io);
            }
        }
        for i in 0..count {
            raw_read_block(sb.journal_start + 1 + i as u32, &mut original)?;
            raw_write_block(get(&header, 16 + i * 8), &original)?;
        }
        clear(&sb)
    }
}

/// Flush metadata before entering the independent snapshot protocol.
/// # Safety
/// Caller owns root FS_LOCK; snapshots must not nest in a metadata operation.
pub(in crate::fs::onyxfs) unsafe fn snapshot_boundary() -> KResult<()> {
    // SAFETY: caller owns the root context and the journal exclusion.
    unsafe {
        if transaction::deferred() {
            return Err(Errno::Busy);
        }
        journal_commit()
    }
}

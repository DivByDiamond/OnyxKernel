//! On-disk journal (circular redo log) for OnyxFS metadata writes.
//!
//! On-disk journal entry layout (one 4096-byte block per entry):
//!   bytes 0..4        : type   (u32) — 0=commit_start, 1=block_write, 2=commit_end
//!   bytes 4..8        : block_num (u32) — target block this entry replays to
//!   bytes 8..4096     : data   (4088 bytes) — block contents to replay
//!
//! The journal is a circular redo log: `journal_log` appends a `block_write`
//! entry containing the NEW block contents before the actual write_block call.
//! `journal_commit` appends a `commit_end` marker. On mount, `journal_recover`
//! scans for a `commit_end`; if found, every preceding `block_write` entry is
//! re-applied to its target block. Incomplete transactions (no commit_end) are
//! discarded.
//!
//! MVP limitation: only the first 4088 bytes of each block are journaled. The
//! last 8 bytes of a 4096-byte block are not protected. This is acceptable
//! because the only metadata that fits in those 8 bytes (rare tail padding of
//! dirent blocks) is not critical for crash recovery.
use super::{G_JOURNAL_HEAD, G_SB, read_block, write_block};
use onyx_core::errno::{Errno, KResult};
use onyx_core::formats::{
    JournalScanStep, ONYFS_BLOCK_SIZE, ONYFS_JOURNAL_BLOCK_WRITE, ONYFS_JOURNAL_COMMIT_END,
    ONYFS_JOURNAL_DATA_SIZE, journal_entry_type, journal_replay_entry, journal_scan_step,
};

/// Append a `block_write` entry to the journal containing the NEW contents
/// of `block_num`. Called BEFORE the actual `write_block` so that a crash
/// between the journal append and the data write leaves a recoverable redo
/// entry on disk. No-op if the filesystem has no journal configured.
///
/// # Safety
///
/// Caller must not invoke onyxfs operations concurrently from multiple harts:
/// G_JOURNAL_HEAD is a non-atomic module global and two racing journal_log
/// calls could overwrite each other's entries.
/// NOTE: no FS-level lock exists; the VFS layer does not serialize callers.
#[inline(never)]
pub(super) unsafe fn journal_log(block_num: u32, data: &[u8; ONYFS_BLOCK_SIZE]) -> KResult<()> {
    // SAFETY: single-threaded onyxfs exclusion (see # Safety); head is
    // bounds-checked against journal_size and `data` is a fixed-size
    // caller-owned block buffer.
    unsafe {
        let sb_ptr = &raw const G_SB;
        let journal_start = (*sb_ptr).journal_start;
        if journal_start == 0 || (*sb_ptr).journal_size == 0 {
            return Ok(());
        }
        let head = G_JOURNAL_HEAD;
        if head >= (*sb_ptr).journal_size {
            // Journal full — caller should have committed by now. Bail out.
            return Err(Errno::NoSpace);
        }
        let mut entry = [0u8; ONYFS_BLOCK_SIZE];
        entry[0..4].copy_from_slice(&ONYFS_JOURNAL_BLOCK_WRITE.to_le_bytes());
        entry[4..8].copy_from_slice(&block_num.to_le_bytes());
        let copy_n = ONYFS_JOURNAL_DATA_SIZE.min(ONYFS_BLOCK_SIZE);
        entry[8..8 + copy_n].copy_from_slice(&data[..copy_n]);
        write_block(journal_start + head, &entry)?;
        G_JOURNAL_HEAD = head + 1;
        Ok(())
    }
}

/// Mark the current transaction as committed by appending a `commit_end`
/// entry. After this, the journal entries are considered durable and will be
/// replayed on the next mount if a crash occurs before the data writes
/// themselves complete. Resets the in-memory journal head so the journal
/// area can be reused for the next transaction.
///
/// Bug #22 fix: also ZERO every journal entry that was part of this
/// transaction. Without this, a subsequent transaction that crashes
/// before writing its own commit_end would leave the new (incomplete)
/// BLOCK_WRITE entries followed by the OLD commit_end marker from this
/// transaction. On next mount, journal_recover would scan past the new
/// entries, hit the old commit_end, and replay BOTH the new (incomplete)
/// entries AND any stale BLOCK_WRITE entries sitting between them and
/// the old commit_end — silently corrupting the filesystem. Zeroing the
/// entries after commit ensures the next transaction always starts with
/// a clean slate on disk.
///
/// # Safety
///
/// Caller must not invoke onyxfs operations concurrently from multiple harts:
/// G_JOURNAL_HEAD is a non-atomic module global and racing commits would
/// interleave their zeroing writes.
/// NOTE: no FS-level lock exists; the VFS layer does not serialize callers.
#[inline(never)]
pub(super) unsafe fn journal_commit() -> KResult<()> {
    // SAFETY: single-threaded onyxfs exclusion (see # Safety); head is
    // bounds-checked against journal_size and only fixed-size stack
    // buffers are written.
    unsafe {
        let sb_ptr = &raw const G_SB;
        let journal_start = (*sb_ptr).journal_start;
        if journal_start == 0 || (*sb_ptr).journal_size == 0 {
            return Ok(());
        }
        let head = G_JOURNAL_HEAD;
        if head == 0 {
            return Ok(()); // nothing to commit
        }
        if head < (*sb_ptr).journal_size {
            let mut entry = [0u8; ONYFS_BLOCK_SIZE];
            entry[0..4].copy_from_slice(&ONYFS_JOURNAL_COMMIT_END.to_le_bytes());
            write_block(journal_start + head, &entry)?;
        }
        G_JOURNAL_HEAD = 0;
        // Zero every entry that was part of this transaction (positions 0..head).
        // The commit_end marker at `head` is left in place for now so a crash
        // mid-zero still results in correct replay on the next mount — once all
        // zeroing is complete, the next transaction's first write will overwrite
        // the commit_end marker.
        let zero = [0u8; ONYFS_BLOCK_SIZE];
        for j in 0..head {
            write_block(journal_start + j, &zero)?;
        }
        // Now zero the commit_end marker too — the transaction is fully
        // completed and the journal area is clean for reuse.
        if head < (*sb_ptr).journal_size {
            write_block(journal_start + head, &zero)?;
        }
        Ok(())
    }
}

/// Replay journal on mount (crash recovery). Scans the journal area for a
/// `commit_end` marker. If found, every preceding `block_write` entry is
/// re-applied to its target block (redo). Incomplete transactions (no
/// `commit_end`) are discarded. The journal is then zeroed so future mounts
/// start with a clean log.
///
/// # Safety
///
/// Must be called only from mount(), i.e. during single-threaded boot init
/// before secondary harts are released, since it rewrites G_JOURNAL_HEAD and
/// the journal area without a lock.
pub unsafe fn journal_recover() -> KResult<()> {
    // SAFETY: boot-time single-threaded exclusion (see # Safety); entry
    // scanning is bounded by journal_size and buffers are fixed-size.
    unsafe {
        let sb_ptr = &raw const G_SB;
        let journal_start = (*sb_ptr).journal_start;
        if journal_start == 0 || (*sb_ptr).journal_size == 0 {
            return Ok(());
        }
        let journal_size = (*sb_ptr).journal_size;
        let mut found_commit = false;
        let mut commit_at: u32 = 0;
        let mut entry = [0u8; ONYFS_BLOCK_SIZE];
        let mut i: u32 = 0;
        while i < journal_size {
            read_block(journal_start + i, &mut entry)?;
            match journal_scan_step(journal_entry_type(&entry)) {
                JournalScanStep::Continue => {}
                JournalScanStep::Commit => {
                    found_commit = true;
                    commit_at = i;
                    break;
                }
                JournalScanStep::Stop => break, // empty slot or garbage — stop scanning
            }
            i += 1;
        }
        if !found_commit {
            G_JOURNAL_HEAD = 0;
            return Ok(());
        }
        // Replay every `block_write` entry before the commit marker.
        for j in 0..commit_at {
            read_block(journal_start + j, &mut entry)?;
            let Some((block_num, data)) = journal_replay_entry(&entry) else {
                continue;
            };
            // Read the current block contents (to preserve the un-journaled tail)
            // and overwrite the first JOURNAL_DATA_SIZE bytes from the entry.
            let mut blk_buf = [0u8; ONYFS_BLOCK_SIZE];
            let _ = read_block(block_num, &mut blk_buf);
            blk_buf[..data.len()].copy_from_slice(&data);
            write_block(block_num, &blk_buf)?;
        }
        // Clear the journal area so future mounts see an empty log.
        let zero = [0u8; ONYFS_BLOCK_SIZE];
        for j in 0..=commit_at {
            write_block(journal_start + j, &zero)?;
        }
        G_JOURNAL_HEAD = 0;
        Ok(())
    }
}

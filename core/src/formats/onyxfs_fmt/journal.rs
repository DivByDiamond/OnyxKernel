//! OnyxFS journal record helpers — pure byte-buffer logic shared by the
//! kernel-side recovery driver (`kernel/src/fs/onyxfs/journal.rs`) and host
//! unit tests (`core/src/formats/tests`). No I/O lives here: callers feed in
//! raw 4096-byte journal entry blocks and get back parse/scan decisions.
//!
//! On-disk journal entry layout (one 4096-byte block per entry):
//!   bytes 0..4        : type   (u32) — 0=commit_start, 1=block_write, 2=commit_end
//!   bytes 4..8        : block_num (u32) — target block this entry replays to
//!   bytes 8..4096     : data   (4088 bytes) — block contents to replay
use super::ONYFS_BLOCK_SIZE;

pub const ONYFS_JOURNAL_COMMIT_START: u32 = 0;
pub const ONYFS_JOURNAL_BLOCK_WRITE: u32 = 1;
pub const ONYFS_JOURNAL_COMMIT_END: u32 = 2;
/// Payload bytes protected per journaled block (last 8 bytes of a block are
/// not covered by the journal — see the kernel-side module docs).
pub const ONYFS_JOURNAL_DATA_SIZE: usize = ONYFS_BLOCK_SIZE - 8;

/// What the recovery scan should do after seeing one journal entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JournalScanStep {
    /// Entry belongs to a transaction — keep scanning forward.
    Continue,
    /// Transaction boundary reached at this entry — replay everything before it.
    Commit,
    /// Empty slot or garbage — stop scanning, discard the incomplete tail.
    Stop,
}

/// Read the u32 type tag from a raw journal entry block.
#[inline]
pub fn journal_entry_type(entry: &[u8]) -> u32 {
    u32::from_le_bytes([entry[0], entry[1], entry[2], entry[3]])
}

/// Classification of a single scanned entry for recovery purposes.
/// Zeroed entries decode as `COMMIT_START` and simply continue the scan,
/// matching the historical kernel behaviour.
#[inline]
pub fn journal_scan_step(entry_type: u32) -> JournalScanStep {
    match entry_type {
        ONYFS_JOURNAL_COMMIT_START | ONYFS_JOURNAL_BLOCK_WRITE => JournalScanStep::Continue,
        ONYFS_JOURNAL_COMMIT_END => JournalScanStep::Commit,
        _ => JournalScanStep::Stop,
    }
}

/// Extract the redo payload `(target_block_num, data)` from a `BLOCK_WRITE`
/// entry. Returns `None` for any other entry kind (they carry no replay data).
pub fn journal_replay_entry(entry: &[u8]) -> Option<(u32, [u8; ONYFS_JOURNAL_DATA_SIZE])> {
    if journal_entry_type(entry) != ONYFS_JOURNAL_BLOCK_WRITE {
        return None;
    }
    let block_num = u32::from_le_bytes([entry[4], entry[5], entry[6], entry[7]]);
    let mut data = [0u8; ONYFS_JOURNAL_DATA_SIZE];
    data.copy_from_slice(&entry[8..8 + ONYFS_JOURNAL_DATA_SIZE]);
    Some((block_num, data))
}

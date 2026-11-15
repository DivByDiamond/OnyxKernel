//! Read compatibility for the original 4088-byte redo records; never written.
use super::super::{raw_read_block, raw_write_block};
use super::format::valid_target;
use onyx_core::errno::KResult;
use onyx_core::formats::{
    JournalScanStep, ONYFS_BLOCK_SIZE, OnyfsSuper, journal_entry_type, journal_replay_entry,
    journal_scan_step,
};

/// # Safety
/// Caller owns FS_LOCK or boot exclusion and a validated mounted volume.
pub(super) unsafe fn recover(sb: &OnyfsSuper) -> KResult<()> {
    // SAFETY: raw I/O bypasses quarantine only for serialized recovery.
    unsafe {
        let mut entry = [0; ONYFS_BLOCK_SIZE];
        let mut commit = None;
        for i in 0..sb.journal_size {
            raw_read_block(sb.journal_start + i, &mut entry)?;
            match journal_scan_step(journal_entry_type(&entry)) {
                JournalScanStep::Continue => {}
                JournalScanStep::Commit => {
                    commit = Some(i);
                    break;
                }
                JournalScanStep::Stop => break,
            }
        }
        if let Some(end) = commit {
            // Check every target before changing any home block.
            for i in 0..end {
                raw_read_block(sb.journal_start + i, &mut entry)?;
                if let Some((target, _)) = journal_replay_entry(&entry) {
                    valid_target(target, sb)?;
                }
            }
            for i in 0..end {
                raw_read_block(sb.journal_start + i, &mut entry)?;
                if let Some((target, data)) = journal_replay_entry(&entry) {
                    let mut block = [0; ONYFS_BLOCK_SIZE];
                    raw_read_block(target, &mut block)?;
                    block[..data.len()].copy_from_slice(&data);
                    raw_write_block(target, &block)?;
                }
            }
        }
        // The new ONJU header supersedes the entire legacy area, including
        // any stale old commit marker. No partial zero sweep is required.
        super::clear(sb)
    }
}

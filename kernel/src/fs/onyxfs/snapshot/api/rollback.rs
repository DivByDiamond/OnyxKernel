//! Public rollback entry point under root VFS exclusion.
use super::super::super::{G_SB, G_SNAPSHOT_QUARANTINED, journal};
use super::super::access;
use super::super::engine;
use super::Disk;
use onyx_core::errno::KResult;

/// Restore a snapshot, retaining a durable pending marker until every full
/// target block has been restored. Mount completes interrupted restores.
/// # Safety
/// Caller holds root filesystem exclusion and no other disk I/O can interleave.
pub unsafe fn snapshot_rollback(id: u32) -> KResult<()> {
    // SAFETY: caller owns the root mount lock across commit and restore.
    unsafe {
        access::access(G_SNAPSHOT_QUARANTINED)?;
        journal::snapshot_boundary()?;
        let mut quarantine = false;
        let result = engine::rollback(&mut Disk, &mut G_SB, id, &mut quarantine);
        G_SNAPSHOT_QUARANTINED = quarantine;
        result
    }
}

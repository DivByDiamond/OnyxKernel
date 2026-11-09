//! Snapshot entry points. The packed engine is tested against memory-backed I/O;
//! kernel callers serialize these globals through the root VFS mount lock.
use super::engine;
use super::format::{Block, Io};
use super::super::{G_SB, G_SNAPSHOT_QUARANTINED, raw_read_block, raw_write_block};
use crate::srv::timer;
use onyx_core::errno::KResult;

mod list;
mod rollback;

pub use list::snapshot_list;
pub use rollback::snapshot_rollback;

pub(super) struct Disk;
impl Io for Disk {
    fn read(&mut self, block: u32, data: &mut Block) -> KResult<()> {
        // SAFETY: entry points require the root VFS filesystem lock.
        unsafe { raw_read_block(block, data) }
    }
    fn write(&mut self, block: u32, data: &Block) -> KResult<()> {
        // SAFETY: entry points require the root VFS filesystem lock.
        unsafe { raw_write_block(block, data) }
    }
}

/// Create a packed snapshot within the existing reserved snapshot area.
/// # Safety
/// Caller holds root filesystem exclusion throughout this operation.
pub unsafe fn snapshot_create(name: &[u8]) -> KResult<u32> {
    // SAFETY: the VFS root mount lock excludes concurrent access to G_SB.
    unsafe {
        super::access::access(G_SNAPSHOT_QUARANTINED)?;
        engine::create(&mut Disk, &mut G_SB, name, timer::jiffies())
    }
}

/// Finish an interrupted snapshot rollback before exposing the mounted root.
/// # Safety
/// Called only during single-threaded mount, before releasing the root to VFS.
pub(in crate::fs::onyxfs) unsafe fn snapshot_recover() -> KResult<()> {
    // SAFETY: mount is single threaded and owns all filesystem globals.
    unsafe {
        let mut quarantine = G_SNAPSHOT_QUARANTINED;
        let result = engine::recover(&mut Disk, &mut G_SB, &mut quarantine);
        G_SNAPSHOT_QUARANTINED = quarantine;
        result
    }
}

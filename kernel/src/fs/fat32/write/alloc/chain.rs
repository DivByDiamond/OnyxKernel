//! Cluster-chain extension and teardown on top of the FAT primitives.
use onyx_core::errno::{Errno, KResult};

use super::fat::{alloc_cluster, free_cluster, write_fat_entry};
use super::super::super::{fat_entry, is_eoc, is_valid_cluster};

/// Extend the chain starting at `start_cluster` by allocating one new
/// cluster and linking it from the current end-of-chain. Returns the new
/// cluster number.
///
/// # Safety
///
/// Caller must not invoke FAT32 I/O concurrently from multiple harts
/// (two racing extend_chain calls could allocate/link the same cluster).
/// NOTE: no FS-level lock exists; the VFS layer does not serialize callers.
pub(in crate::fs::fat32::write) unsafe fn extend_chain(start_cluster: u32) -> KResult<u32> {
    // SAFETY: single-threaded FAT32 exclusion (see # Safety); chain walking
    // is hop-capped and every next-cluster value is validated before use.
    unsafe {
        let new_cluster = alloc_cluster()?;
        // Walk to the EOC of the existing chain.
        let mut buf = [0u8; 512];
        let mut cur = start_cluster;
        let mut hop = 0u32;
        const MAX_HOPS: u32 = 65536;
        loop {
            hop += 1;
            if hop >= MAX_HOPS {
                // Defensive: chain is corrupt or way too long.
                let _ = free_cluster(new_cluster);
                return Err(Errno::Io);
            }
            let next = fat_entry(cur, &mut buf);
            if is_eoc(next) {
                // Link cur → new_cluster.
                write_fat_entry(cur, new_cluster)?;
                return Ok(new_cluster);
            }
            if !is_valid_cluster(next) {
                let _ = free_cluster(new_cluster);
                return Err(Errno::Io);
            }
            cur = next;
        }
    }
}

/// Free every cluster in the chain starting at `start_cluster`.
///
/// # Safety
///
/// Caller must not invoke FAT32 I/O concurrently from multiple harts;
/// `start_cluster` must be a valid data cluster (callers check
/// is_valid_cluster), and chain hops are validated and hop-capped below.
pub(in crate::fs::fat32::write) unsafe fn free_chain(start_cluster: u32) -> KResult<()> {
    // SAFETY: single-threaded FAT32 exclusion (see # Safety); all raw access
    // is delegated to fat_entry/free_cluster under validated cluster numbers.
    unsafe {
        let mut cur = start_cluster;
        let mut buf = [0u8; 512];
        let mut hop = 0u32;
        const MAX_HOPS: u32 = 65536;
        loop {
            hop += 1;
            if hop >= MAX_HOPS {
                return Err(Errno::Io);
            }
            if !is_valid_cluster(cur) {
                return Ok(());
            }
            let next = fat_entry(cur, &mut buf);
            let _ = free_cluster(cur);
            if is_eoc(next) || !is_valid_cluster(next) {
                return Ok(());
            }
            cur = next;
        }
    }
}

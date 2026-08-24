//! Block- and FAT-level write operations: sector IO, FAT entry patching
//! (mirrored across every BPB_NumFATs copy), cluster alloc/free, and
//! chain extend/free.
use onyx_core::errno::{Errno, KResult};

use super::super::{
    FAT32_EOC, G_DEV, G_FAT_SZ, G_NUM_FATS, G_RESVD, fat_entry, is_eoc, is_valid_cluster, read_sec,
};

/// Total number of clusters in the FAT (data region capacity).
/// Calculated from G_FAT_SZ, G_SPC, and 512-byte sectors.
unsafe fn total_clusters() -> u32 {
    unsafe {
        // FAT32: each FAT entry is 4 bytes. A FAT sector holds 128 entries.
        // total entries = G_FAT_SZ * 128. This is an upper bound; the real
        // data region may have fewer usable clusters.
        G_FAT_SZ * 128
    }
}

/// Patch the low 28 bits of an existing FAT entry, preserving its top 4
/// bits as required by the FAT32 spec. Pure — unit-tested in tests.rs.
pub(super) fn patch_fat_value(existing: u32, value: u32) -> u32 {
    (existing & 0xF000_0000) | (value & 0x0FFF_FFFF)
}

/// Write the 4-byte FAT entry for `cluster` with the given `value` into
/// EVERY FAT copy on the volume (BPB_NumFATs). Each copy is read,
/// patched, and written back independently so a stale mirror can never
/// resurrect a freed cluster or drop a fresh allocation.
pub(super) unsafe fn write_fat_entry(cluster: u32, value: u32) -> KResult<()> {
    unsafe {
        let fat_off = cluster as u64 * 4;
        let sec_in_fat = fat_off / 512;
        let off = (fat_off % 512) as usize;
        let num_fats = G_NUM_FATS.max(1);
        let mut buf = [0u8; 512];
        for i in 0..num_fats {
            let lba = G_RESVD as u64 + i as u64 * G_FAT_SZ as u64 + sec_in_fat;
            read_sec(lba, &mut buf)?;
            let existing = u32::from_le_bytes([buf[off], buf[off + 1], buf[off + 2], buf[off + 3]]);
            let bytes = patch_fat_value(existing, value).to_le_bytes();
            buf[off..off + 4].copy_from_slice(&bytes);
            write_sec(lba, &buf)?;
        }
        Ok(())
    }
}

/// Write a 512-byte sector to the disk.
pub(super) unsafe fn write_sec(lba: u64, buf: &[u8; 512]) -> KResult<()> {
    unsafe { crate::drivers::virtio_req::write(G_DEV, lba, buf.as_ptr()) }
}

/// Write `buf` to the given sector offset within a cluster's data area.
pub(super) unsafe fn write_cluster_sector(
    cluster: u32,
    sector_in_cluster: u32,
    buf: &[u8; 512],
) -> KResult<()> {
    unsafe {
        let lba = super::super::cluster_to_lba(cluster) + sector_in_cluster as u64;
        write_sec(lba, buf)
    }
}

/// Find a free cluster by scanning the FAT starting from cluster 2.
/// Returns the cluster number, or Err(ENOSPC) if the FAT is full.
pub(super) unsafe fn alloc_cluster() -> KResult<u32> {
    unsafe {
        let total = total_clusters();
        let mut buf = [0u8; 512];
        let entries_per_sec: u32 = 128;
        let sectors = total.div_ceil(entries_per_sec);
        for s in 0..sectors {
            let lba = G_RESVD as u64 + s as u64;
            if read_sec(lba, &mut buf).is_err() {
                continue;
            }
            let base = s * entries_per_sec;
            for i in 0..entries_per_sec {
                let cluster = base + i;
                if cluster < 2 || cluster >= total {
                    continue;
                }
                let off = (i * 4) as usize;
                let v = u32::from_le_bytes([buf[off], buf[off + 1], buf[off + 2], buf[off + 3]])
                    & 0x0FFF_FFFF;
                if v == 0 {
                    // Free. Mark as EOC in every FAT copy (write_fat_entry
                    // mirrors across all of them).
                    write_fat_entry(cluster, FAT32_EOC)?;
                    return Ok(cluster);
                }
            }
        }
        Err(Errno::NoSpace)
    }
}

/// Free a cluster (mark FAT entry as 0).
pub(super) unsafe fn free_cluster(cluster: u32) -> KResult<()> {
    unsafe { write_fat_entry(cluster, 0) }
}

/// Extend the chain starting at `start_cluster` by allocating one new
/// cluster and linking it from the current end-of-chain. Returns the new
/// cluster number.
pub(super) unsafe fn extend_chain(start_cluster: u32) -> KResult<u32> {
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
pub(super) unsafe fn free_chain(start_cluster: u32) -> KResult<()> {
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

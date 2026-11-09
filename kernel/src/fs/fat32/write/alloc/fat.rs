//! Block- and FAT-level write operations: sector IO, FAT entry patching
//! (mirrored across every BPB_NumFATs copy), cluster alloc/free and the
//! FSINFO hint update.
use onyx_core::errno::{Errno, KResult};

use super::super::super::{
    FAT32_EOC, G_DEV, G_FAT_SZ, G_FSINFO, G_NUM_FATS, G_RESVD, read_sec,
};

/// FSINFO sector lead signature (offset 0).
const FSINFO_LEAD_SIG: u32 = 0x4161_5252;
/// FSINFO structure signature (offset 484).
const FSINFO_STRUCT_SIG: u32 = 0x6141_7272;
/// "Unknown" value for the FSINFO free-cluster count.
const FSINFO_UNKNOWN: u32 = 0xFFFF_FFFF;

/// Total number of clusters in the FAT (data region capacity).
/// Calculated from G_FAT_SZ, G_SPC, and 512-byte sectors.
///
/// # Safety
///
/// Reads only the mount-initialized G_FAT_SZ global; unsafe by convention
/// to require the mounted context. No raw dereferences.
unsafe fn total_clusters() -> u32 {
    // SAFETY: only reads the G_FAT_SZ global set by mount().
    unsafe {
        // FAT32: each FAT entry is 4 bytes. A FAT sector holds 128 entries.
        // total entries = G_FAT_SZ * 128. This is an upper bound; the real
        // data region may have fewer usable clusters.
        G_FAT_SZ * 128
    }
}

/// Patch the low 28 bits of an existing FAT entry, preserving its top 4
/// bits as required by the FAT32 spec. Pure — unit-tested in tests.rs.
pub(in crate::fs::fat32::write) fn patch_fat_value(existing: u32, value: u32) -> u32 {
    (existing & 0xF000_0000) | (value & 0x0FFF_FFFF)
}

/// Write the 4-byte FAT entry for `cluster` with the given `value` into
/// EVERY FAT copy on the volume (BPB_NumFATs). Each copy is read,
/// patched, and written back independently so a stale mirror can never
/// resurrect a freed cluster or drop a fresh allocation.
///
/// # Safety
///
/// Caller must not invoke FAT32 I/O concurrently from multiple harts;
/// `cluster` must be a valid FAT index (callers check is_valid_cluster), so
/// the byte offset stays within G_FAT_SZ sectors and off+3 < 512 because
/// cluster*4 is 4-aligned relative to the sector size.
pub(in crate::fs::fat32::write) unsafe fn write_fat_entry(cluster: u32, value: u32) -> KResult<()> {
    // SAFETY: single-threaded FAT32 exclusion (see # Safety); buf is a valid
    // 512-byte stack buffer and the 4-byte patch is in-bounds per above.
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
///
/// # Safety
///
/// Caller must not invoke FAT32 I/O concurrently from multiple harts; mount()
/// must have initialized G_DEV. `buf` is a caller-owned, fully initialized
/// 512-byte buffer.
pub(in crate::fs::fat32::write) unsafe fn write_sec(lba: u64, buf: &[u8; 512]) -> KResult<()> {
    // SAFETY: buf is a valid, initialized 512-byte buffer; G_DEV was set by
    // mount() (single-threaded boot init).
    unsafe { crate::drivers::virtio_req::write(G_DEV, lba, buf.as_ptr()) }
}

/// Write `buf` to the given sector offset within a cluster's data area.
///
/// # Safety
///
/// Caller must not invoke FAT32 I/O concurrently from multiple harts;
/// `cluster` must be a valid data cluster so cluster_to_lba cannot
/// underflow, and `sector_in_cluster` must be < G_SPC (callers iterate
/// 0..G_SPC). `buf` is a caller-owned 512-byte buffer.
pub(in crate::fs::fat32::write) unsafe fn write_cluster_sector(
    cluster: u32,
    sector_in_cluster: u32,
    buf: &[u8; 512],
) -> KResult<()> {
    // SAFETY: see # Safety; buf is a valid 512-byte buffer.
    unsafe {
        let lba = super::super::super::cluster_to_lba(cluster) + sector_in_cluster as u64;
        write_sec(lba, buf)
    }
}

/// Find a free cluster by scanning the FAT starting from cluster 2.
/// Returns the cluster number, or Err(ENOSPC) if the FAT is full.
///
/// # Safety
///
/// Caller must not invoke FAT32 I/O concurrently from multiple harts:
/// concurrent allocators could hand out the same free cluster twice.
/// NOTE: no FS-level lock exists; the VFS layer does not serialize callers.
pub(in crate::fs::fat32::write) unsafe fn alloc_cluster() -> KResult<u32> {
    // SAFETY: single-threaded FAT32 exclusion (see # Safety); the scan is
    // bounded by total_clusters() and cluster indices are range-checked
    // before each entry read (off = i*4 < 512 for i < 128).
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
                    // FSINFO is only a hint: a failure here must not fail the
                    // allocation (the FAT itself is already authoritative).
                    fsinfo_adjust(-1, cluster.saturating_add(1)).ok();
                    return Ok(cluster);
                }
            }
        }
        Err(Errno::NoSpace)
    }
}

/// Free a cluster (mark FAT entry as 0).
///
/// # Safety
///
/// Caller must not invoke FAT32 I/O concurrently from multiple harts;
/// `cluster` must be a valid FAT index (delegated contract of
/// write_fat_entry).
pub(in crate::fs::fat32::write) unsafe fn free_cluster(cluster: u32) -> KResult<()> {
    // SAFETY: delegates entirely to write_fat_entry, whose contract covers
    // the raw access for a validated cluster number. The FSINFO hint update
    // is best-effort and must not fail the free.
    unsafe {
        write_fat_entry(cluster, 0)?;
        fsinfo_adjust(1, cluster).ok();
        Ok(())
    }
}

/// Adjust the FSINFO free-cluster count by `delta` and set the next-free hint.
/// A missing, malformed, or "unknown" count is left as-is: the field is only
/// a hint, and an unknown value tells checkers to recount instead of trusting
/// a number this driver did not compute from scratch.
///
/// # Safety
///
/// Caller must not invoke FAT32 I/O concurrently from multiple harts;
/// `G_FSINFO` must come from a completed `mount()`.
unsafe fn fsinfo_adjust(delta: i32, next_free: u32) -> KResult<()> {
    // SAFETY: single-hart FAT32 exclusion; sector offsets stay inside the
    // 512-byte buffer and `G_FSINFO` is a validated BPB field.
    unsafe {
        let sector = u64::from(G_FSINFO);
        if G_FSINFO == 0 || G_FSINFO == 0xFFFF {
            return Ok(());
        }
        let mut buf = [0u8; 512];
        read_sec(sector, &mut buf)?;
        let lead = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
        let structure = u32::from_le_bytes([buf[484], buf[485], buf[486], buf[487]]);
        if lead != FSINFO_LEAD_SIG || structure != FSINFO_STRUCT_SIG {
            return Ok(());
        }
        let free = u32::from_le_bytes([buf[488], buf[489], buf[490], buf[491]]);
        if free != FSINFO_UNKNOWN {
            let updated = if delta >= 0 {
                free.saturating_add(delta as u32)
            } else {
                free.saturating_sub(delta.unsigned_abs())
            };
            buf[488..492].copy_from_slice(&updated.to_le_bytes());
        }
        buf[492..496].copy_from_slice(&next_free.to_le_bytes());
        write_sec(sector, &buf)
    }
}

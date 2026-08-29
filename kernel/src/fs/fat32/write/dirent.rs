//! Directory-entry primitives: free-slot allocation and entry writes.
use onyx_core::errno::{Errno, KResult};

use super::super::{
    DIR_ENTRY_SIZE, ENTRIES_PER_SECTOR, G_SPC, fat_entry, is_eoc, is_valid_cluster,
    read_cluster_sector,
};
use super::fat::{alloc_cluster, write_cluster_sector, write_fat_entry};

/// Find a free 32-byte directory entry slot in the directory rooted at
/// `dir_cluster`. Returns (cluster_of_slot, sector_in_cluster, entry_index).
/// If the directory is full and is the root, returns ENOSPC.
/// If the directory is full and is a subdirectory, extends it by one cluster.
///
/// # Safety
///
/// Caller must not invoke FAT32 I/O concurrently from multiple harts;
/// `dir_cluster` must be a valid data cluster and is re-validated on every
/// chain hop. Entry offsets are ei*32 with ei < 16, staying in-sector.
pub(super) unsafe fn find_free_dirent_slot(
    dir_cluster: u32,
    out_cluster: &mut u32,
    out_sec: &mut u32,
    out_idx: &mut usize,
) -> KResult<()> {
    // SAFETY: single-threaded FAT32 exclusion (see # Safety); all buffer
    // access is delegated to the bounds-checked read_cluster_sector/
    // fat_entry/write helpers.
    unsafe {
        let mut buf = [0u8; 512];
        let mut cluster = dir_cluster;
        let mut hop = 0u32;
        const MAX_HOPS: u32 = 65536;
        loop {
            hop += 1;
            if hop >= MAX_HOPS {
                return Err(Errno::Io);
            }
            for si in 0..G_SPC {
                read_cluster_sector(cluster, si, &mut buf)?;
                for ei in 0..ENTRIES_PER_SECTOR {
                    let off = ei * DIR_ENTRY_SIZE;
                    // 0x00 = end of directory, 0xE5 = deleted. Both are reusable.
                    if buf[off] == 0x00 || buf[off] == 0xE5 {
                        *out_cluster = cluster;
                        *out_sec = si;
                        *out_idx = ei;
                        return Ok(());
                    }
                }
            }
            let next = fat_entry(cluster, &mut buf);
            if is_eoc(next) {
                // End of directory chain — extend if this is a subdirectory
                // (root dir on FAT32 is also a cluster chain, so extension
                // works for both).
                let new_cluster = alloc_cluster()?;
                // Zero the new cluster first.
                let zero = [0u8; 512];
                for si in 0..G_SPC {
                    let _ = write_cluster_sector(new_cluster, si, &zero);
                }
                // Link.
                write_fat_entry(cluster, new_cluster)?;
                *out_cluster = new_cluster;
                *out_sec = 0;
                *out_idx = 0;
                return Ok(());
            }
            if !is_valid_cluster(next) {
                return Err(Errno::Io);
            }
            cluster = next;
        }
    }
}

/// Write a 32-byte directory entry into the slot identified by
/// (cluster, sector_in_cluster, entry_index).
///
/// # Safety
///
/// Caller must not invoke FAT32 I/O concurrently from multiple harts;
/// `cluster` must be a valid data cluster and (sector_in_cluster,
/// entry_index) must identify a real slot in the directory (callers obtain
/// these from find_free_dirent_slot), so entry_index*32 < 512.
pub(super) unsafe fn write_dirent(
    cluster: u32,
    sector_in_cluster: u32,
    entry_index: usize,
    entry: &[u8; 32],
) -> KResult<()> {
    // SAFETY: single-threaded FAT32 exclusion (see # Safety); the 32-byte
    // copy stays in bounds because entry_index < 16 for a 512-byte sector.
    unsafe {
        let mut buf = [0u8; 512];
        read_cluster_sector(cluster, sector_in_cluster, &mut buf)?;
        let off = entry_index * DIR_ENTRY_SIZE;
        buf[off..off + 32].copy_from_slice(entry);
        write_cluster_sector(cluster, sector_in_cluster, &buf)
    }
}

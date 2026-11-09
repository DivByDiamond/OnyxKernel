//! On-disk size synchronisation for open FAT32 files.
//!
//! A FAT32 fd carries the file's first cluster and its parent directory
//! cluster (VfsFd::ino / VfsFd::dir): there is no inode number to update a
//! size field through, so a growing write must find the dirent that points
//! at that cluster and patch its 4-byte size.
use onyx_core::errno::{Errno, KResult};

use super::super::super::{
    DIR_ENTRY_SIZE, ENTRIES_PER_SECTOR, G_SPC, fat_entry, is_eoc, is_valid_cluster,
    read_cluster_sector,
};
use super::fat::write_cluster_sector;

/// Directory-entry attribute byte values needed to skip non-file entries.
const ATTR_LFN: u8 = 0x0F;
/// Offset of the 4-byte file size inside a 32-byte dirent.
const SIZE_OFFSET: usize = 28;
/// Offset of the low 16 bits of the first cluster.
const CLUSTER_LO: usize = 26;
/// Offset of the high 16 bits of the first cluster (FAT32 only).
const CLUSTER_HI: usize = 20;
/// Entry name byte that marks end-of-directory.
const DIRENT_END: u8 = 0x00;
/// Entry name byte that marks a deleted entry.
const DIRENT_DELETED: u8 = 0xE5;

/// Write `new_size` into the dirent of the file whose first cluster is
/// `file_cluster`, scanning the directory at `dir_cluster` (and its chain).
///
/// Returns `Err(NoEnt)` when no live dirent points at that cluster — callers
/// treat this as "cannot attribute the size" rather than silently diverging.
///
/// # Safety
///
/// Caller must not invoke FAT32 I/O concurrently from multiple harts (the
/// module globals are unsynchronized); `dir_cluster` must be a valid data
/// cluster and is re-validated on every chain hop. `file_cluster` must be a
/// live data cluster (0 is rejected: an empty foreign file cannot be
/// identified by cluster alone).
pub unsafe fn set_size(dir_cluster: u32, file_cluster: u32, new_size: u32) -> KResult<()> {
    // SAFETY: single-threaded FAT32 exclusion (see # Safety); every buffer
    // access is ei*32 with ei < 16 inside a 512-byte sector buffer.
    unsafe {
        if !is_valid_cluster(file_cluster) {
            return Err(Errno::NoEnt);
        }
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
                    let attr = buf[off + 11];
                    if attr == ATTR_LFN {
                        continue;
                    }
                    if buf[off] == DIRENT_END {
                        return Err(Errno::NoEnt);
                    }
                    if buf[off] == DIRENT_DELETED {
                        continue;
                    }
                    let lo = u16::from_le_bytes([buf[off + CLUSTER_LO], buf[off + CLUSTER_LO + 1]]);
                    let hi = u16::from_le_bytes([buf[off + CLUSTER_HI], buf[off + CLUSTER_HI + 1]]);
                    if ((hi as u32) << 16) | lo as u32 != file_cluster {
                        continue;
                    }
                    let bytes = new_size.to_le_bytes();
                    buf[off + SIZE_OFFSET..off + SIZE_OFFSET + 4].copy_from_slice(&bytes);
                    return write_cluster_sector(cluster, si, &buf);
                }
            }
            let next = fat_entry(cluster, &mut buf);
            if is_eoc(next) || !is_valid_cluster(next) {
                return Err(Errno::NoEnt);
            }
            cluster = next;
        }
    }
}

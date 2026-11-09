//! Directory-walk resume caching for sequential readdir calls.
use super::super::super::{
    ENTRIES_PER_SECTOR, G_DATA_LBA, G_DEV, G_SPC, fat_entry, is_eoc, is_valid_cluster,
};
use super::super::scan::{G_WALK, WalkPos};

/// Record the resume point after a successful decode of the entry at
/// (`cluster`, `si`, `ei`) with cursor `entry_idx`, then hand the entry
/// back. The next slot is computed eagerly — including the FAT hop when
/// the entry closed out a cluster — so the resume filter in
/// `readdir_entry` stays a plain bounds check; when the chain ends right
/// there the cache is cleared instead of pointing past the directory.
pub(super) fn save_resume(
    buf: &mut [u8; 512],
    dir: u32,
    cluster: u32,
    si: u32,
    ei: usize,
    entry_idx: u32,
    ino: u32,
    dtype: u8,
) -> Option<(u32, u8)> {
    // SAFETY: FAT32_LOCK (with_mount) serializes all G_WALK/G_* access —
    // the same exclusion as every other fat32 entry point; fat_entry
    // re-validates the cluster it returns and the resume filter in
    // readdir_entry bounds-checks si/ei before they are used.
    unsafe {
        let next = if ei + 1 < ENTRIES_PER_SECTOR {
            Some((cluster, si, ei + 1))
        } else if si + 1 < G_SPC {
            Some((cluster, si + 1, 0))
        } else {
            // Chain hop: this FAT read reuses buf, which the caller is done
            // with at this point (it is returning the entry).
            let n = fat_entry(cluster, buf);
            if is_eoc(n) || !is_valid_cluster(n) {
                None
            } else {
                Some((n, 0, 0))
            }
        };
        // The volume key stops this entry from matching a different mount.
        G_WALK = next.map(|(cluster, si, ei)| WalkPos {
            volume: G_DEV,
            lba: G_DATA_LBA,
            dir,
            cluster,
            si,
            ei,
            cursor: entry_idx + 1,
        });
        Some((ino, dtype))
    }
}

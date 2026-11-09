//! Directory scan: locate a live entry by its 8.3 alias or its long name.
use onyx_core::errno::{Errno, KResult};

use super::super::{
    ATTR_DIRECTORY, ATTR_LFN, DIR_ENTRY_SIZE, ENTRIES_PER_SECTOR, G_SPC, fat_entry, fat32_name_8_3,
    is_eoc, is_valid_cluster, read_cluster_sector,
};
use super::lfn::{LfnChain, name_eq};

/// Walk a directory cluster chain looking for `component`, matching either
/// the 11-byte 8.3 name or the LFN chain that precedes it (case-insensitive).
/// Chain traversal is hop-capped (MAX_HOPS) and every cluster is validated
/// against the FAT before following it.
///
/// # Safety
///
/// Caller must not invoke FAT32 I/O concurrently from multiple harts (the
/// module globals are unsynchronized); `dir_cluster` must be a valid data
/// cluster; `buf` is a caller-owned 512-byte scratch buffer reused across
/// sector reads.
pub(crate) unsafe fn scan_dir_entries(
    dir_cluster: u32,
    component: &[u8],
    out_cluster: &mut u32,
    out_size: &mut u32,
    is_dir: &mut bool,
    buf: &mut [u8; 512],
) -> KResult<()> {
    // SAFETY: see # Safety; entry offsets are ei*32 with ei < 16, so all
    // buffer reads stay within the 512-byte sector.
    unsafe {
        let needle = fat32_name_8_3(component);
        let mut chain = LfnChain::new();
        let mut long_name = [0u8; 255];
        let mut cluster = dir_cluster;
        if !is_valid_cluster(cluster) {
            return Err(Errno::NoEnt);
        }
        let mut hop = 0u32;
        const MAX_HOPS: u32 = 65536;
        loop {
            if hop >= MAX_HOPS {
                return Err(Errno::Io);
            }
            hop += 1;
            for si in 0..G_SPC {
                read_cluster_sector(cluster, si, buf)?;
                for ei in 0..ENTRIES_PER_SECTOR {
                    let off = ei * DIR_ENTRY_SIZE;
                    let attr = buf[off + 11];
                    if attr == ATTR_LFN {
                        chain.push(&buf[off..off + DIR_ENTRY_SIZE]);
                        continue;
                    }
                    if buf[off] == 0 {
                        return Err(Errno::NoEnt);
                    }
                    if buf[off] == 0xE5 {
                        chain.reset();
                        continue;
                    }
                    let mut entry = [0u8; 11];
                    entry.copy_from_slice(&buf[off..off + 11]);
                    let short_match = entry == needle;
                    let long_match = chain.belongs_to(&entry)
                        && chain
                            .decode(&mut long_name)
                            .is_some_and(|len| name_eq(&long_name[..len], component));
                    if short_match || long_match {
                        let cluster_lo = u16::from_le_bytes([buf[off + 26], buf[off + 27]]);
                        let cluster_hi = u16::from_le_bytes([buf[off + 20], buf[off + 21]]);
                        *out_cluster = ((cluster_hi as u32) << 16) | cluster_lo as u32;
                        *out_size = u32::from_le_bytes([
                            buf[off + 28],
                            buf[off + 29],
                            buf[off + 30],
                            buf[off + 31],
                        ]);
                        *is_dir = (attr & ATTR_DIRECTORY) != 0;
                        return Ok(());
                    }
                    chain.reset();
                }
            }
            let next = fat_entry(cluster, buf);
            if is_eoc(next) {
                return Err(Errno::NoEnt);
            }
            if !is_valid_cluster(next) {
                return Err(Errno::Io);
            }
            cluster = next;
        }
    }
}

/// Sequential-readdir resume point: the slot right after the entry the
/// previous `readdir_entry` call returned, plus that entry's cursor index.
/// FAT32 directories are cluster chains with no random access, so without
/// this every restart re-walked the whole prefix from the first cluster —
/// listing N entries cost O(N²) sector reads (each skip re-read the FAT
/// and every passed-over data sector from disk).
///
/// Keyed by volume geometry (`G_DEV` + `G_DATA_LBA`) as well as the
/// directory cluster: secondary mounts swap those globals in and out under
/// `FAT32_LOCK`, and two volumes may legitimately reuse the same cluster
/// numbers.
#[derive(Clone, Copy)]
pub(super) struct WalkPos {
    pub(super) volume: usize,
    pub(super) lba: u32,
    pub(super) dir: u32,
    pub(super) cluster: u32,
    pub(super) si: u32,
    pub(super) ei: usize,
    pub(super) cursor: u32,
}

/// Resume cache for sequential readdir; guarded by `FAT32_LOCK` (taken by
/// `vfs::with_mount` around every runtime FAT32 operation, boot paths are
/// single-hart).
pub(super) static mut G_WALK: Option<WalkPos> = None;

/// Drop the readdir resume cache. Every operation that changes directory
/// contents or volume geometry (`mount`, `create`, `unlink`) must call
/// this: a resumed position would otherwise point into a shifted layout.
/// A missed invalidation can only mis-list entries for one iteration —
/// the walk still bounds-checks every cluster and sector it reads.
pub(crate) fn invalidate_walk() {
    // SAFETY: all FAT32 I/O runs under FAT32_LOCK, the same exclusion that
    // guards the G_WALK reads in readdir_entry.
    unsafe {
        G_WALK = None;
    }
}

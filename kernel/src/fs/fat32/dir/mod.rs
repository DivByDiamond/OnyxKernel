//! FAT32 directory operations: path resolution and directory iteration.
pub(crate) mod lfn;
mod path;
mod read;
mod scan;

use super::lfn::LfnChain;
use super::{
    ATTR_DIRECTORY, ATTR_LFN, DIR_ENTRY_SIZE, ENTRIES_PER_SECTOR, G_DATA_LBA, G_DEV, G_SPC,
    fat_entry, is_eoc, is_valid_cluster, read_cluster_sector,
};
use scan::{G_WALK, WalkPos};

pub(crate) use scan::invalidate_walk;
pub(crate) use scan::scan_dir_entries;

pub use path::*;
pub use read::*;

/// Read one directory entry by index (stateful cursor maintained by VFS).
/// Writes the long name when a valid LFN chain precedes the entry, otherwise
/// the 8.3-derived lowercase name (NUL-terminated), into `name_out`, and
/// returns the entry's cluster number plus its d_type (DT_DIR/DT_REG from
/// the attribute byte), or None past end-of-directory.
///
/// # Safety
///
/// `name_out` must be writable for `name_len` bytes and stay valid for the
/// call (the syscall layer translates user buffers before reaching fs/);
/// every write below is capped at name_len - 1 plus a NUL. Caller must not
/// invoke FAT32 I/O concurrently from multiple harts.
pub unsafe fn readdir_entry(
    dir_cluster: u32,
    entry_idx: u32,
    name_out: *mut u8,
    name_len: usize,
) -> Option<(u32, u8)> {
    // SAFETY: see # Safety for name_out validity - out_pos is checked against
    // name_len before each byte write; cluster numbers are validated via
    // is_valid_cluster before each chain hop.
    unsafe {
        if !is_valid_cluster(dir_cluster) {
            return None;
        }
        let mut buf = [0u8; 512];
        let mut chain = LfnChain::new();
        let mut long_name = [0u8; 255];
        let mut cluster = dir_cluster;
        let mut cursor = 0u32;
        let mut si_from = 0u32;
        let mut ei_from = 0usize;
        let mut first = true;
        // Sequential-access fast path: resume where the previous call for
        // this directory stopped instead of re-walking the whole prefix
        // (the common getdents/SYS_readdir loop). The bounds checks make a
        // stale or cross-volume entry fall back to a full walk rather than
        // an out-of-range sector read.
        // SAFETY: G_WALK/G_DEV/G_DATA_LBA are only read under FAT32_LOCK
        // (with_mount), the same exclusion guarding every fat32 global.
        let (walk, volume, lba) = (G_WALK, G_DEV, G_DATA_LBA);
        if let Some(w) = walk.filter(|w| {
            w.volume == volume
                && w.lba == lba
                && w.dir == dir_cluster
                && entry_idx >= w.cursor
                && is_valid_cluster(w.cluster)
                && w.si < G_SPC
                && w.ei < ENTRIES_PER_SECTOR
        }) {
            cluster = w.cluster;
            si_from = w.si;
            ei_from = w.ei;
            cursor = w.cursor;
        }
        let mut hop = 0u32;
        const MAX_HOPS: u32 = 65536;
        loop {
            if hop >= MAX_HOPS {
                return None;
            }
            hop += 1;
            let si_start = if first { si_from } else { 0 };
            for si in si_start..G_SPC {
                if read_cluster_sector(cluster, si, &mut buf).is_err() {
                    return None;
                }
                let ei_start = if first && si == si_from { ei_from } else { 0 };
                for ei in ei_start..ENTRIES_PER_SECTOR {
                    let off = ei * DIR_ENTRY_SIZE;
                    let attr = buf[off + 11];
                    let dtype = if attr & ATTR_DIRECTORY != 0 {
                        crate::fs::DT_DIR
                    } else {
                        crate::fs::DT_REG
                    };
                    if attr == ATTR_LFN {
                        chain.push(&buf[off..off + DIR_ENTRY_SIZE]);
                        continue;
                    }
                    if buf[off] == 0 {
                        return None;
                    }
                    if buf[off] == 0xE5 {
                        chain.reset();
                        continue;
                    }
                    if cursor < entry_idx {
                        cursor += 1;
                        chain.reset();
                        continue;
                    }
                    let mut short_name = [0u8; 11];
                    short_name.copy_from_slice(&buf[off..off + 11]);
                    let long_len = if chain.belongs_to(&short_name) {
                        chain.decode(&mut long_name)
                    } else {
                        None
                    };
                    chain.reset();
                    if let Some(long_len) = long_len {
                        let mut out_pos = 0usize;
                        for &byte in long_name.iter().take(long_len) {
                            if out_pos >= name_len.saturating_sub(1) {
                                break;
                            }
                            name_out.add(out_pos).write(byte);
                            out_pos += 1;
                        }
                        if out_pos < name_len {
                            name_out.add(out_pos).write(0);
                        }
                        let cluster_lo = u16::from_le_bytes([buf[off + 26], buf[off + 27]]);
                        let cluster_hi = u16::from_le_bytes([buf[off + 20], buf[off + 21]]);
                        let cluster_num = ((cluster_hi as u32) << 16) | cluster_lo as u32;
                        return save_resume(
                            &mut buf,
                            dir_cluster,
                            cluster,
                            si,
                            ei,
                            entry_idx,
                            if cluster_num == 0 {
                                dir_cluster
                            } else {
                                cluster_num
                            },
                            dtype,
                        );
                    }
                    let mut out_pos = 0usize;
                    let mut base_end = 8usize;
                    for i in (0..8).rev() {
                        if buf[off + i] != b' ' {
                            base_end = i + 1;
                            break;
                        }
                    }
                    for i in 0..base_end {
                        if out_pos >= name_len.saturating_sub(1) {
                            break;
                        }
                        let c = buf[off + i];
                        name_out.add(out_pos).write(if c.is_ascii_uppercase() {
                            c + 32
                        } else {
                            c
                        });
                        out_pos += 1;
                    }
                    let mut ext_end = 11usize;
                    for i in (8..11).rev() {
                        if buf[off + i] != b' ' {
                            ext_end = i + 1;
                            break;
                        }
                    }
                    if ext_end > 8 {
                        if out_pos < name_len.saturating_sub(1) {
                            name_out.add(out_pos).write(b'.');
                            out_pos += 1;
                        }
                        for i in 8..ext_end {
                            if out_pos >= name_len.saturating_sub(1) {
                                break;
                            }
                            let c = buf[off + i];
                            name_out.add(out_pos).write(if c.is_ascii_uppercase() {
                                c + 32
                            } else {
                                c
                            });
                            out_pos += 1;
                        }
                    }
                    if out_pos < name_len {
                        name_out.add(out_pos).write(0);
                    }
                    let cluster_lo = u16::from_le_bytes([buf[off + 26], buf[off + 27]]);
                    let cluster_hi = u16::from_le_bytes([buf[off + 20], buf[off + 21]]);
                    let cluster_num = ((cluster_hi as u32) << 16) | cluster_lo as u32;
                    return save_resume(
                        &mut buf,
                        dir_cluster,
                        cluster,
                        si,
                        ei,
                        entry_idx,
                        if cluster_num == 0 {
                            dir_cluster
                        } else {
                            cluster_num
                        },
                        dtype,
                    );
                }
            }
            first = false;
            let next = fat_entry(cluster, &mut buf);
            if is_eoc(next) {
                return None;
            }
            if !is_valid_cluster(next) {
                return None;
            }
            cluster = next;
        }
    }
}

/// Record the resume point after a successful decode of the entry at
/// (`cluster`, `si`, `ei`) with cursor `entry_idx`, then hand the entry
/// back. The next slot is computed eagerly — including the FAT hop when
/// the entry closed out a cluster — so the resume filter in
/// `readdir_entry` stays a plain bounds check; when the chain ends right
/// there the cache is cleared instead of pointing past the directory.
fn save_resume(
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

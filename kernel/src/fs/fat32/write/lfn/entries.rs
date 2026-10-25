//! LFN entry generation and append-at-end-of-directory.
use onyx_core::errno::{Errno, KResult};

use super::super::super::dir::lfn::{
    LFN_CHARS_PER_ENTRY, LFN_MAX_CHARS, LFN_MAX_ENTRIES, lfn_checksum,
};
use super::super::super::{
    ATTR_DIRECTORY, ATTR_LFN, DIR_ENTRY_SIZE, ENTRIES_PER_SECTOR, G_SPC, fat_entry, is_eoc,
    is_valid_cluster, read_cluster_sector,
};
use super::super::fat::{alloc_cluster, write_cluster_sector, write_fat_entry};
use super::name::unique_short;
use super::utf16::utf16_units;

/// Entry name byte that marks end-of-directory.
const DIRENT_END: u8 = 0x00;
/// Sequence-number flag on the physically first LFN entry.
const LFN_SEQ_LAST: u8 = 0x40;
/// Byte offsets of the 13 UTF-16 units inside a 32-byte LFN entry.
const LFN_OFFSETS: [usize; LFN_CHARS_PER_ENTRY] = [1, 3, 5, 7, 9, 14, 16, 18, 20, 22, 24, 28, 30];
/// Checksum byte inside an LFN entry.
const LFN_CHECKSUM_OFFSET: usize = 13;
/// Directory entries one creation may need: 20 LFN fragments + short entry.
const MAX_CREATE_ENTRIES: usize = LFN_MAX_ENTRIES + 1;

/// Append `entries` at the first end-of-directory marker, extending the
/// directory chain when it is full.
///
/// # Safety
///
/// Caller must not invoke FAT32 I/O concurrently from multiple harts;
/// `dir_cluster` must be a valid directory data cluster.
unsafe fn append_entries(dir_cluster: u32, entries: &[[u8; 32]]) -> KResult<()> {
    // SAFETY: single-threaded FAT32 exclusion; every slot index is bounded by
    // ENTRIES_PER_SECTOR and every cluster is validated before use.
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
                    if buf[ei * DIR_ENTRY_SIZE] != DIRENT_END {
                        continue;
                    }
                    return write_run(cluster, si, ei, entries);
                }
            }
            let next = fat_entry(cluster, &mut buf);
            if is_eoc(next) {
                let new_cluster = alloc_cluster()?;
                let zero = [0u8; 512];
                for si in 0..G_SPC {
                    write_cluster_sector(new_cluster, si, &zero)?;
                }
                write_fat_entry(cluster, new_cluster)?;
                return write_run(new_cluster, 0, 0, entries);
            }
            if !is_valid_cluster(next) {
                return Err(Errno::Io);
            }
            cluster = next;
        }
    }
}

/// Write a run of entries starting at one slot, crossing sector and cluster
/// boundaries (allocating a cluster when the chain ends).
///
/// # Safety
///
/// Same contract as `append_entries`; (cluster, sector, index) must identify
/// a real slot (from `append_entries`) so index*32 < 512.
unsafe fn write_run(
    mut cluster: u32,
    mut sector: u32,
    mut index: usize,
    entries: &[[u8; 32]],
) -> KResult<()> {
    // SAFETY: single-threaded FAT32 exclusion; slot arithmetic stays inside
    // the 512-byte buffer and cluster numbers are validated per hop.
    unsafe {
        let mut buf = [0u8; 512];
        read_cluster_sector(cluster, sector, &mut buf)?;
        let mut dirty = false;
        for entry in entries {
            buf[index * DIR_ENTRY_SIZE..(index + 1) * DIR_ENTRY_SIZE].copy_from_slice(entry);
            dirty = true;
            index += 1;
            if index < ENTRIES_PER_SECTOR {
                continue;
            }
            write_cluster_sector(cluster, sector, &buf)?;
            dirty = false;
            index = 0;
            sector += 1;
            if sector < G_SPC {
                read_cluster_sector(cluster, sector, &mut buf)?;
                continue;
            }
            let mut scratch = [0u8; 512];
            let next = fat_entry(cluster, &mut scratch);
            sector = 0;
            if is_eoc(next) {
                let new_cluster = alloc_cluster()?;
                let zero = [0u8; 512];
                for si in 0..G_SPC {
                    write_cluster_sector(new_cluster, si, &zero)?;
                }
                write_fat_entry(cluster, new_cluster)?;
                cluster = new_cluster;
            } else if is_valid_cluster(next) {
                cluster = next;
            } else {
                return Err(Errno::Io);
            }
            read_cluster_sector(cluster, sector, &mut buf)?;
        }
        if dirty {
            write_cluster_sector(cluster, sector, &buf)?;
        }
        Ok(())
    }
}

/// Create the LFN chain and short entry for `name` in `dir_cluster`, pointing
/// at `first_cluster`.
///
/// # Safety
///
/// Caller must not invoke FAT32 I/O concurrently from multiple harts;
/// `dir_cluster` must be a valid data cluster and `first_cluster` a freshly
/// allocated cluster that the caller frees if this returns an error.
pub(crate) unsafe fn create_long(
    dir_cluster: u32,
    name: &[u8],
    first_cluster: u32,
    is_dir: bool,
) -> KResult<()> {
    // SAFETY: single-threaded FAT32 exclusion; all buffer indices are derived
    // from the fixed entry layout and clamped to the arrays below.
    unsafe {
        let short = unique_short(dir_cluster, name)?;
        let checksum = lfn_checksum(&short);
        let mut units = [0u16; LFN_MAX_CHARS];
        let chars = utf16_units(name, &mut units);
        if chars == 0 {
            return Err(Errno::Inval);
        }
        let fragments = chars.div_ceil(LFN_CHARS_PER_ENTRY);
        if fragments == 0 || fragments > LFN_MAX_ENTRIES {
            return Err(Errno::Inval);
        }
        let mut entries = [[0u8; 32]; MAX_CREATE_ENTRIES];
        for seq in (1..=fragments).rev() {
            let entry = &mut entries[fragments - seq];
            entry[0] = seq as u8 | if seq == fragments { LFN_SEQ_LAST } else { 0 };
            entry[11] = ATTR_LFN;
            entry[12] = 0;
            entry[LFN_CHECKSUM_OFFSET] = checksum;
            for (i, &off) in LFN_OFFSETS.iter().enumerate() {
                let at = (seq - 1) * LFN_CHARS_PER_ENTRY + i;
                let unit = match at.cmp(&chars) {
                    core::cmp::Ordering::Less => units[at],
                    core::cmp::Ordering::Equal => 0x0000,
                    core::cmp::Ordering::Greater => 0xFFFF,
                };
                entry[off..off + 2].copy_from_slice(&unit.to_le_bytes());
            }
        }
        let entry = &mut entries[fragments];
        entry[..11].copy_from_slice(&short);
        entry[11] = if is_dir { ATTR_DIRECTORY } else { 0 };
        entry[20..22].copy_from_slice(&((first_cluster >> 16) as u16).to_le_bytes());
        entry[26..28].copy_from_slice(&(first_cluster as u16).to_le_bytes());
        entry[28..32].copy_from_slice(&0u32.to_le_bytes());
        append_entries(dir_cluster, &entries[..fragments + 1])
    }
}

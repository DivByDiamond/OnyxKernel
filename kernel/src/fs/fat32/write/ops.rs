//! Directory-level file operations: create, unlink, size update.
//!
//! Ordering invariant (crash safety): metadata that makes content
//! reachable is written BEFORE clusters are freed. Unlink tombstones the
//! dirent first and only then frees the chain — a crash in between leaks
//! clusters (benign) instead of leaving a live dirent pointing at
//! clusters other files may reuse (cross-file corruption).
use onyx_core::errno::{Errno, KResult};

use super::super::{
    ATTR_DIRECTORY, ATTR_LFN, DIR_ENTRY_SIZE, ENTRIES_PER_SECTOR, G_ROOT_CLUSTER, G_SPC, fat_entry,
    fat32_name_8_3, is_eoc, is_valid_cluster, read_cluster_sector, scan_dir_entries,
};
use super::entry::dirent::{find_free_dirent_slot, write_dirent};
use super::alloc::fat::{alloc_cluster, write_cluster_sector};
use super::alloc::chain::free_chain;

/// Create a new file in `dir_cluster` with the given 8.3 name.
/// Returns the new file's first cluster (or 0 for an empty file).
///
/// # Safety
///
/// Caller must not invoke FAT32 I/O concurrently from multiple harts
/// (module globals and FAT/dirent state are unsynchronized). `dir_cluster`
/// must be a valid data cluster; `name` is length-checked in-line and
/// converted to an 8.3 short name (no LFN support).
pub unsafe fn create(dir_cluster: u32, name: &[u8], is_dir: bool) -> KResult<u32> {
    // SAFETY: single-threaded FAT32 exclusion (see # Safety); all raw access
    // is delegated to the bounds-checked scan/alloc/dirent helpers.
    unsafe {
        // The new dirent (and any LFN run) shifts slot positions: drop any
        // readdir resume point so the next sequential read re-walks.
        super::super::dir::invalidate_walk();
        // Long names are allowed here; the LFN path below rejects anything
        // over 255 bytes (20 fragments) and 8.3 aliases are truncated.
        if name.is_empty() || name.len() > 255 {
            return Err(Errno::Inval);
        }
        let name_83 = fat32_name_8_3(name);
        // Check if it already exists (the scanner matches the 8.3 alias and
        // the LFN name, so neither form can be duplicated).
        let mut existing_cluster = 0u32;
        let mut existing_size = 0u32;
        let mut existing_is_dir = false;
        let mut buf = [0u8; 512];
        let already_exists = scan_dir_entries(
            dir_cluster,
            name,
            &mut existing_cluster,
            &mut existing_size,
            &mut existing_is_dir,
            &mut buf,
        )
        .is_ok();
        if already_exists {
            return Err(Errno::Exist);
        }
        // Allocate one cluster for the new file/dir.
        let first_cluster = alloc_cluster()?;
        // Zero the new cluster.
        let zero = [0u8; 512];
        for si in 0..G_SPC {
            let _ = write_cluster_sector(first_cluster, si, &zero);
        }
        // If creating a directory, populate "." and ".." entries.
        if is_dir {
            write_dot_entries(dir_cluster, first_cluster)?;
        }
        // Names that 8.3 cannot represent get an LFN chain. That path owns its
        // own dirent allocation, so it must release the cluster on failure.
        if super::lfn::needs_lfn(name) {
            if let Err(e) = super::lfn::create_long(dir_cluster, name, first_cluster, is_dir) {
                let _ = free_chain(first_cluster);
                return Err(e);
            }
            return Ok(first_cluster);
        }
        // Find a free dirent slot. On failure the allocated cluster must
        // be released — otherwise it leaks as an orphaned EOC island no
        // file will ever reference or free.
        let mut slot_cluster = 0u32;
        let mut slot_sec = 0u32;
        let mut slot_idx = 0usize;
        if let Err(e) =
            find_free_dirent_slot(dir_cluster, &mut slot_cluster, &mut slot_sec, &mut slot_idx)
        {
            let _ = free_chain(first_cluster);
            return Err(e);
        }
        // Build the new dirent.
        let mut entry = [0u8; 32];
        entry[..11].copy_from_slice(&name_83);
        entry[11] = if is_dir { ATTR_DIRECTORY } else { 0 };
        entry[20..22].copy_from_slice(&((first_cluster >> 16) as u16).to_le_bytes());
        entry[26..28].copy_from_slice(&(first_cluster as u16).to_le_bytes());
        entry[28..32].copy_from_slice(&0u32.to_le_bytes());
        write_dirent(slot_cluster, slot_sec, slot_idx, &entry)?;
        Ok(first_cluster)
    }
}

/// Populate "." and ".." in a freshly allocated directory cluster.
///
/// # Safety
///
/// Caller must not invoke FAT32 I/O concurrently from multiple harts;
/// `first_cluster` must be a freshly allocated valid data cluster and
/// `dir_cluster` a valid directory cluster (from create()'s alloc path).
unsafe fn write_dot_entries(dir_cluster: u32, first_cluster: u32) -> KResult<()> {
    // SAFETY: single-threaded FAT32 exclusion (see # Safety); the 64-byte
    // dot-entry patch stays within the 512-byte sector buffer.
    unsafe {
        let mut dot = [0u8; 32];
        dot[..11].copy_from_slice(b".          ");
        dot[11] = ATTR_DIRECTORY;
        dot[20..22].copy_from_slice(&((first_cluster >> 16) as u16).to_le_bytes());
        dot[26..28].copy_from_slice(&(first_cluster as u16).to_le_bytes());
        dot[28..32].copy_from_slice(&0u32.to_le_bytes());
        let mut dotdot = [0u8; 32];
        dotdot[..11].copy_from_slice(b"..         ");
        dotdot[11] = ATTR_DIRECTORY;
        let parent_cluster = if dir_cluster == G_ROOT_CLUSTER {
            0
        } else {
            dir_cluster
        };
        dotdot[20..22].copy_from_slice(&((parent_cluster >> 16) as u16).to_le_bytes());
        dotdot[26..28].copy_from_slice(&(parent_cluster as u16).to_le_bytes());
        dotdot[28..32].copy_from_slice(&0u32.to_le_bytes());
        let mut cbuf = [0u8; 512];
        read_cluster_sector(first_cluster, 0, &mut cbuf)?;
        cbuf[0..32].copy_from_slice(&dot);
        cbuf[32..64].copy_from_slice(&dotdot);
        write_cluster_sector(first_cluster, 0, &cbuf)
    }
}

/// Update the size field in a directory entry. Looks up `name` in
/// `dir_cluster` and writes the new size (4 bytes, little-endian) into
/// the dirent at offset 28.
///
/// # Safety
///
/// Caller must not invoke FAT32 I/O concurrently from multiple harts;
/// `dir_cluster` must be a valid data cluster and is re-validated on every
/// chain hop below.
pub unsafe fn update_size(dir_cluster: u32, name: &[u8], new_size: u32) -> KResult<()> {
    // SAFETY: single-threaded FAT32 exclusion (see # Safety); entry offsets
    // are ei*32 with ei < 16, staying within the 512-byte sector buffer.
    unsafe {
        if name.is_empty() {
            return Err(Errno::Inval);
        }
        let needle = fat32_name_8_3(name);
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
                    if buf[off] == 0x00 {
                        return Err(Errno::NoEnt);
                    }
                    if buf[off] == 0xE5 {
                        continue;
                    }
                    let mut entry = [0u8; 11];
                    entry.copy_from_slice(&buf[off..off + 11]);
                    if entry == needle {
                        let size_bytes = new_size.to_le_bytes();
                        buf[off + 28] = size_bytes[0];
                        buf[off + 29] = size_bytes[1];
                        buf[off + 30] = size_bytes[2];
                        buf[off + 31] = size_bytes[3];
                        return write_cluster_sector(cluster, si, &buf);
                    }
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

//! FAT32 write support — extends the read-only driver with file write,
//! create, unlink, and chain extension.
//!
//! All operations go through the FAT to allocate/free clusters and chain
//! them into existing files. LFN (long file name) is NOT supported —
//! all new files are created with 8.3 short names (case-insensitive
//! lookup is handled by fat32_name_8_3 in helpers.rs).
//!
//! Concurrency: there is no per-FS lock; the kernel is single-threaded
//! with respect to virtio_blk requests at the time of writing. When SMP
//! is fully wired for FS I/O, callers must hold a global FAT32 lock.

use core::ptr;
use onyx_core::errno::{Errno, KResult};

use super::helpers::fat32_name_8_3;
use super::{
    ATTR_DIRECTORY, ATTR_LFN, DIR_ENTRY_SIZE, ENTRIES_PER_SECTOR, FAT32_EOC, G_DEV,
    G_FAT_SZ, G_RESVD, G_ROOT_CLUSTER, G_SPC, cluster_to_lba, fat_entry, is_eoc, is_valid_cluster,
    read_cluster_sector, read_sec,
};

/// Total number of clusters in the FAT (data region capacity).
/// Calculated from G_FAT_SZ, G_SPC, and 512-byte sectors.
unsafe fn total_clusters() -> u32 { unsafe {
    // FAT32: each FAT entry is 4 bytes. A FAT sector holds 128 entries.
    // total entries = G_FAT_SZ * 128. This is an upper bound; the real
    // data region may have fewer usable clusters.
    G_FAT_SZ * 128
}}

/// Write the 4-byte FAT entry for `cluster` with the given `value`.
/// Reads the containing sector, patches the entry, writes the sector back.
unsafe fn write_fat_entry(cluster: u32, value: u32) -> KResult<()> { unsafe {
    let fat_off = cluster as u64 * 4;
    let fat_lba = G_RESVD as u64 + fat_off / 512;
    let mut buf = [0u8; 512];
    read_sec(fat_lba, &mut buf)?;
    let off = (fat_off % 512) as usize;
    // Preserve top 4 bits per FAT32 spec — but we only write the low 28 bits.
    let existing = u32::from_le_bytes([buf[off], buf[off + 1], buf[off + 2], buf[off + 3]]);
    let new_value = (existing & 0xF000_0000) | (value & 0x0FFF_FFFF);
    let bytes = new_value.to_le_bytes();
    buf[off] = bytes[0];
    buf[off + 1] = bytes[1];
    buf[off + 2] = bytes[2];
    buf[off + 3] = bytes[3];
    write_sec(fat_lba, &buf)
}}

/// Write a 512-byte sector to the disk.
unsafe fn write_sec(lba: u64, buf: &[u8; 512]) -> KResult<()> { unsafe {
    crate::drivers::virtio_req::write(G_DEV, lba, buf.as_ptr())
}}

/// Write a full cluster (G_SPC sectors) from a 512-byte sector buffer
/// repeated, OR more commonly, write `data` (≤ cluster size) starting
/// at the given sector offset within the cluster.
unsafe fn write_cluster_sector(
    cluster: u32,
    sector_in_cluster: u32,
    buf: &[u8; 512],
) -> KResult<()> { unsafe {
    let lba = cluster_to_lba(cluster) + sector_in_cluster as u64;
    write_sec(lba, buf)
}}

/// Find a free cluster by scanning the FAT starting from cluster 2.
/// Returns the cluster number, or Err(ENOSPC) if the FAT is full.
unsafe fn alloc_cluster() -> KResult<u32> { unsafe {
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
                // Free. Mark as EOC and write back.
                write_fat_entry(cluster, FAT32_EOC)?;
                // Also mirror to the second FAT copy if present.
                if G_FAT_SZ > 0 {
                    let second_fat_lba = G_RESVD as u64 + G_FAT_SZ as u64 + s as u64;
                    let mut buf2 = [0u8; 512];
                    if read_sec(second_fat_lba, &mut buf2).is_ok() {
                        buf2[off] = buf[off];
                        buf2[off + 1] = buf[off + 1];
                        buf2[off + 2] = buf[off + 2];
                        buf2[off + 3] = buf[off + 3];
                        let _ = write_sec(second_fat_lba, &buf2);
                    }
                }
                return Ok(cluster);
            }
        }
    }
    Err(Errno::NoSpace)
}}

/// Free a cluster (mark FAT entry as 0).
unsafe fn free_cluster(cluster: u32) -> KResult<()> { unsafe {
    write_fat_entry(cluster, 0)
}}

/// Extend the chain starting at `start_cluster` by allocating one new
/// cluster and linking it from the current end-of-chain. Returns the new
/// cluster number.
unsafe fn extend_chain(start_cluster: u32) -> KResult<u32> { unsafe {
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
}}

/// Find a free 32-byte directory entry slot in the directory rooted at
/// `dir_cluster`. Returns (cluster_of_slot, sector_in_cluster, entry_index).
/// If the directory is full and is the root, returns ENOSPC.
/// If the directory is full and is a subdirectory, extends it by one cluster.
unsafe fn find_free_dirent_slot(
    dir_cluster: u32,
    out_cluster: &mut u32,
    out_sec: &mut u32,
    out_idx: &mut usize,
) -> KResult<()> { unsafe {
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
}}

/// Write a 32-byte directory entry into the slot identified by
/// (cluster, sector_in_cluster, entry_index).
unsafe fn write_dirent(
    cluster: u32,
    sector_in_cluster: u32,
    entry_index: usize,
    entry: &[u8; 32],
) -> KResult<()> { unsafe {
    let mut buf = [0u8; 512];
    read_cluster_sector(cluster, sector_in_cluster, &mut buf)?;
    let off = entry_index * DIR_ENTRY_SIZE;
    buf[off..off + 32].copy_from_slice(entry);
    write_cluster_sector(cluster, sector_in_cluster, &buf)
}}

/// Create a new file in `dir_cluster` with the given 8.3 name.
/// Returns the new file's first cluster (or 0 for an empty file).
pub unsafe fn create(dir_cluster: u32, name: &[u8], is_dir: bool) -> KResult<u32> { unsafe {
    if name.is_empty() || name.len() > 12 {
        return Err(Errno::Inval);
    }
    let name_83 = fat32_name_8_3(name);
    // Check if it already exists.
    let mut existing_cluster = 0u32;
    let mut existing_size = 0u32;
    let mut existing_is_dir = false;
    let mut buf = [0u8; 512];
    let already_exists = super::helpers::scan_dir_entries(
        dir_cluster,
        &name_83,
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
        write_cluster_sector(first_cluster, 0, &cbuf)?;
    }
    // Find a free dirent slot.
    let mut slot_cluster = 0u32;
    let mut slot_sec = 0u32;
    let mut slot_idx = 0usize;
    find_free_dirent_slot(dir_cluster, &mut slot_cluster, &mut slot_sec, &mut slot_idx)?;
    // Build the new dirent.
    let mut entry = [0u8; 32];
    entry[..11].copy_from_slice(&name_83);
    entry[11] = if is_dir { ATTR_DIRECTORY } else { 0 };
    entry[20..22].copy_from_slice(&((first_cluster >> 16) as u16).to_le_bytes());
    entry[26..28].copy_from_slice(&(first_cluster as u16).to_le_bytes());
    entry[28..32].copy_from_slice(&0u32.to_le_bytes());
    write_dirent(slot_cluster, slot_sec, slot_idx, &entry)?;
    Ok(first_cluster)
}}

/// Mark a directory entry as deleted (0xE5) and free its clusters.
/// Returns Ok(()) if found and deleted, Err(NoEnt) if not found.
pub unsafe fn unlink(dir_cluster: u32, name: &[u8]) -> KResult<()> { unsafe {
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
                let mut entry_name = [0u8; 11];
                entry_name.copy_from_slice(&buf[off..off + 11]);
                if entry_name == needle {
                    // Found. Get the file's first cluster.
                    let cluster_lo = u16::from_le_bytes([buf[off + 26], buf[off + 27]]) as u32;
                    let cluster_hi = u16::from_le_bytes([buf[off + 20], buf[off + 21]]) as u32;
                    let file_cluster = (cluster_hi << 16) | cluster_lo;
                    // Free all clusters in the chain.
                    if file_cluster != 0 {
                        free_chain(file_cluster)?;
                    }
                    // Mark entry as deleted.
                    buf[off] = 0xE5;
                    write_cluster_sector(cluster, si, &buf)?;
                    return Ok(());
                }
            }
        }
        let next = fat_entry(cluster, &mut buf);
        if is_eoc(next) || !is_valid_cluster(next) {
            return Err(Errno::NoEnt);
        }
        cluster = next;
    }
}}

/// Free every cluster in the chain starting at `start_cluster`.
unsafe fn free_chain(start_cluster: u32) -> KResult<()> { unsafe {
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
}}

/// Write `len` bytes from `buf` into the file starting at offset `off`.
/// The file's first cluster is `first_cluster` (must already exist —
/// if 0, a new chain is allocated). Extends the chain as needed.
/// Returns the number of bytes written.
pub unsafe fn write(first_cluster: u32, buf: *const u8, off: u32, len: u32) -> KResult<u32> { unsafe {
    if len == 0 {
        return Ok(0);
    }
    let sector_size = 512u32;
    let cluster_bytes = G_SPC * sector_size;

    // If first_cluster is 0, we need to allocate one. This shouldn't
    // happen for a properly-created file (create() allocates one cluster),
    // but we handle it defensively.
    let mut cluster = first_cluster;
    if cluster == 0 {
        cluster = alloc_cluster()?;
        let zero = [0u8; 512];
        for si in 0..G_SPC {
            let _ = write_cluster_sector(cluster, si, &zero);
        }
    }

    let mut sec_buf = [0u8; 512];
    let mut cur_pos = off as u64;
    let end_byte = off as u64 + len as u64;
    let mut total_written: u32 = 0;
    let mut cluster_base: u64 = 0;

    loop {
        let cluster_end = cluster_base + cluster_bytes as u64;
        if cur_pos < cluster_end && cur_pos < end_byte {
            let rel_start = (cur_pos - cluster_base) as u32;
            let want = (end_byte - cur_pos) as u32;
            let avail = cluster_bytes - rel_start;
            let mut remain = want.min(avail);
            let copied_before = total_written;
            let mut sec_idx = rel_start / sector_size;
            let mut sec_off = rel_start % sector_size;
            while remain > 0 {
                let in_sec = sector_size - sec_off;
                let chunk = remain.min(in_sec) as usize;
                // Read-modify-write: load sector, patch, store.
                read_cluster_sector(cluster, sec_idx, &mut sec_buf)?;
                ptr::copy_nonoverlapping(
                    buf.add(total_written as usize),
                    sec_buf.as_mut_ptr().add(sec_off as usize),
                    chunk,
                );
                write_cluster_sector(cluster, sec_idx, &sec_buf)?;
                total_written += chunk as u32;
                remain -= chunk as u32;
                sec_off = 0;
                sec_idx += 1;
            }
            cur_pos += (total_written - copied_before) as u64;
            if cur_pos >= end_byte {
                return Ok(total_written);
            }
        }
        cluster_base += cluster_bytes as u64;
        // Advance to the next cluster, allocating if needed.
        let next = fat_entry(cluster, &mut sec_buf);
        if is_eoc(next) {
            // Need to extend the chain for more data.
            let _ = extend_chain(cluster);
            let new_next = fat_entry(cluster, &mut sec_buf);
            if is_eoc(new_next) || !is_valid_cluster(new_next) {
                return Ok(total_written);
            }
            cluster = new_next;
        } else if !is_valid_cluster(next) {
            return Ok(total_written);
        } else {
            cluster = next;
        }
    }
}}

/// Truncate a file's cluster chain to release clusters past `keep_bytes`.
/// The dirent's size field is NOT updated here — caller's responsibility
/// (VFS layer does this via stat / setattr).
pub unsafe fn truncate_chain(first_cluster: u32, keep_bytes: u64) -> KResult<()> { unsafe {
    if first_cluster == 0 || !is_valid_cluster(first_cluster) {
        return Ok(());
    }
    let cluster_bytes = (G_SPC * 512) as u64;
    let keep_clusters = keep_bytes.div_ceil(cluster_bytes);
    if keep_clusters == 0 {
        // Free everything.
        return free_chain(first_cluster);
    }
    let mut buf = [0u8; 512];
    let mut cur = first_cluster;
    for _ in 1..keep_clusters {
        let next = fat_entry(cur, &mut buf);
        if is_eoc(next) || !is_valid_cluster(next) {
            return Ok(());
        }
        cur = next;
    }
    // `cur` is the last kept cluster. Free the rest.
    let after = fat_entry(cur, &mut buf);
    if is_eoc(after) {
        return Ok(());
    }
    if is_valid_cluster(after) {
        free_chain(after)?;
    }
    // Mark `cur` as EOC.
    write_fat_entry(cur, FAT32_EOC)?;
    Ok(())
}}

/// Update the size field in a directory entry. Looks up `name` in
/// `dir_cluster` and writes the new size (4 bytes, little-endian) into
/// the dirent at offset 28.
pub unsafe fn update_size(dir_cluster: u32, name: &[u8], new_size: u32) -> KResult<()> { unsafe {
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
}}

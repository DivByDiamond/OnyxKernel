//! File-data write operations: byte-range writes with chain extension,
//! and truncation of a cluster chain.
use core::ptr;
use onyx_core::errno::KResult;

use super::super::{FAT32_EOC, G_SPC, fat_entry, is_eoc, is_valid_cluster, read_cluster_sector};
use super::fat::{alloc_cluster, extend_chain, free_chain, write_cluster_sector};

/// Write `len` bytes from `buf` into the file starting at offset `off`.
/// The file's first cluster is `first_cluster` (must already exist —
/// if 0, a new chain is allocated). Extends the chain as needed.
/// Returns the number of bytes written.
///
/// # Safety
///
/// `buf` must point to `len` bytes of valid, initialized memory that stays
/// valid for the duration of the call (the syscall layer translates user
/// buffers before reaching fs/). Caller must not invoke FAT32 I/O
/// concurrently from multiple harts (module globals are unsynchronized).
pub unsafe fn write(first_cluster: u32, buf: *const u8, off: u32, len: u32) -> KResult<u32> {
    // SAFETY: see # Safety for buf validity; total_written only advances by
    // clamped chunk sizes so writes stay within the caller's buffer, and
    // cluster numbers are validated on every FAT hop.
    unsafe {
        if len == 0 {
            return Ok(0);
        }
        let sector_size = 512u32;
        let cluster_bytes = G_SPC * sector_size;

        // If first_cluster is 0, we need to allocate one. This shouldn't
        // happen for a properly-created file (create() allocates one
        // cluster), but we handle it defensively.
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
    }
}

/// Truncate a file's cluster chain to release clusters past `keep_bytes`.
/// The dirent's size field is NOT updated here — caller's responsibility
/// (VFS layer does this via stat / setattr).
///
/// # Safety
///
/// Caller must not invoke FAT32 I/O concurrently from multiple harts;
/// `first_cluster` must be a valid data cluster (0 or invalid values are
/// rejected below), and every FAT hop is validated via is_valid_cluster.
pub unsafe fn truncate_chain(first_cluster: u32, keep_bytes: u64) -> KResult<()> {
    // SAFETY: single-threaded FAT32 exclusion (see # Safety); all raw access
    // is delegated to fat_entry/free_chain/write_fat_entry under validated
    // cluster numbers.
    unsafe {
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
        super::fat::write_fat_entry(cur, FAT32_EOC)
    }
}

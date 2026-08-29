use core::ptr;
use onyx_core::errno::KResult;

use super::super::{G_SPC, fat_entry, is_eoc, is_valid_cluster, read_cluster_sector};

/// Read `len` bytes at `off` from the cluster chain starting at `cluster`,
/// following the FAT until EOC. Returns the number of bytes actually read.
///
/// # Safety
///
/// `buf` must be writable for at least the bytes this call reports; the
/// syscall layer translates user buffers before reaching fs/. Caller must
/// not invoke FAT32 I/O concurrently from multiple harts; `cluster` must be
/// a valid data cluster and is re-validated on every chain hop below.
pub unsafe fn read(cluster: u32, buf: *mut u8, off: u32, len: u32) -> KResult<u32> {
    // SAFETY: see # Safety for buf validity; total_copied only advances by
    // the clamped chunk sizes, so writes stay within the caller's buffer,
    // and sec_buf access stays within its 512-byte bounds.
    unsafe {
        if len == 0 || cluster == 0 {
            return Ok(0);
        }
        let sector_size = 512u32;
        let cluster_bytes = G_SPC * sector_size;
        let end_byte = off as u64 + len as u64;

        let mut sec_buf = [0u8; 512];
        let mut cluster = cluster;
        let mut cur_pos = off as u64;
        let mut total_copied: u32 = 0;
        let mut cluster_base: u64 = 0;

        loop {
            let cluster_end = cluster_base + cluster_bytes as u64;
            if cur_pos < cluster_end && cur_pos < end_byte {
                let rel_start = (cur_pos - cluster_base) as u32;
                let want = (end_byte - cur_pos) as u32;
                let avail = cluster_bytes - rel_start;
                let mut remain = want.min(avail);
                let copied_before = total_copied;
                let mut sec_idx = rel_start / sector_size;
                let mut sec_off = rel_start % sector_size;
                while remain > 0 {
                    let in_sec = sector_size - sec_off;
                    let chunk = remain.min(in_sec) as usize;
                    read_cluster_sector(cluster, sec_idx, &mut sec_buf)?;
                    ptr::copy_nonoverlapping(
                        sec_buf.as_ptr().add(sec_off as usize),
                        buf.add(total_copied as usize),
                        chunk,
                    );
                    total_copied += chunk as u32;
                    remain -= chunk as u32;
                    sec_off = 0;
                    sec_idx += 1;
                }
                cur_pos += (total_copied - copied_before) as u64;
                if cur_pos >= end_byte {
                    return Ok(total_copied);
                }
            }
            cluster_base += cluster_bytes as u64;
            let next = fat_entry(cluster, &mut sec_buf);
            if is_eoc(next) {
                return Ok(total_copied);
            }
            if !is_valid_cluster(next) {
                return Ok(total_copied);
            }
            cluster = next;
        }
    }
}

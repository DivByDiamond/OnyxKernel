use core::ptr;
use onyx_core::errno::{Errno, KResult};

use super::super::{fat_entry, is_eoc, is_valid_cluster, read_cluster_sector, G_SPC};

pub unsafe fn read(cluster: u32, buf: *mut u8, off: u32, len: u32) -> KResult<u32> {
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

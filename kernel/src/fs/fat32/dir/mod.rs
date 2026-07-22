use core::ptr;
use onyx_core::errno::{Errno, KResult};

use super::{
    fat32_name_8_3, fat_entry, is_eoc, is_valid_cluster, read_cluster_sector, scan_dir_entries,
    ATTR_DIRECTORY, ATTR_LFN, DIR_ENTRY_SIZE, ENTRIES_PER_SECTOR, FAT32_EOC, G_ROOT_CLUSTER, G_SPC,
};

unsafe fn lookup_component(
    dir_cluster: u32,
    component: &[u8],
    out_cluster: &mut u32,
    out_size: &mut u32,
    out_is_dir: &mut bool,
) -> KResult<()> {
    let needle = fat32_name_8_3(component);
    let mut buf = [0u8; 512];
    scan_dir_entries(
        dir_cluster,
        &needle,
        out_cluster,
        out_size,
        out_is_dir,
        &mut buf,
    )
}

pub unsafe fn lookup(path: &[u8], out_cluster: &mut u32, out_size: &mut u32) -> KResult<()> {
    if path.is_empty() || path == b"/" || path == b"." {
        *out_cluster = G_ROOT_CLUSTER;
        *out_size = 0;
        return Ok(());
    }
    let mut current_cluster = G_ROOT_CLUSTER;
    let mut pos = 0usize;
    let len = path.len();
    loop {
        while pos < len && path[pos] == b'/' {
            pos += 1;
        }
        if pos >= len {
            break;
        }
        let start = pos;
        while pos < len && path[pos] != b'/' {
            pos += 1;
        }
        let component = &path[start..pos];
        let mut found_cluster = 0u32;
        let mut found_size = 0u32;
        let mut is_dir = false;
        lookup_component(
            current_cluster,
            component,
            &mut found_cluster,
            &mut found_size,
            &mut is_dir,
        )?;
        if pos >= len {
            *out_cluster = found_cluster;
            *out_size = found_size;
            return Ok(());
        }
        if !is_dir {
            return Err(Errno::NoEnt);
        }
        current_cluster = found_cluster;
    }
    *out_cluster = current_cluster;
    *out_size = 0;
    Ok(())
}

pub unsafe fn readdir_entry(
    dir_cluster: u32,
    entry_idx: u32,
    name_out: *mut u8,
    name_len: usize,
) -> Option<u32> {
    if !is_valid_cluster(dir_cluster) {
        return None;
    }
    let mut buf = [0u8; 512];
    let mut cluster = dir_cluster;
    let mut cursor = 0u32;
    let mut hop = 0u32;
    const MAX_HOPS: u32 = 65536;
    loop {
        if hop >= MAX_HOPS {
            return None;
        }
        hop += 1;
        for si in 0..G_SPC {
            if read_cluster_sector(cluster, si, &mut buf).is_err() {
                return None;
            }
            for ei in 0..ENTRIES_PER_SECTOR {
                let off = ei * DIR_ENTRY_SIZE;
                let attr = buf[off + 11];
                if attr == ATTR_LFN {
                    continue;
                }
                if buf[off] == 0 {
                    return None;
                }
                if buf[off] == 0xE5 {
                    continue;
                }
                if cursor < entry_idx {
                    cursor += 1;
                    continue;
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
                    name_out
                        .add(out_pos)
                        .write(if c >= b'A' && c <= b'Z' { c + 32 } else { c });
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
                        name_out.add(out_pos).write(if c >= b'A' && c <= b'Z' {
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
                return Some(if cluster_num == 0 {
                    dir_cluster
                } else {
                    cluster_num
                });
            }
        }
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

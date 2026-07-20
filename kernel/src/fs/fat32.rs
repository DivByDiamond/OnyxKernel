//! FAT32 read-only driver.
use crate::drivers::{virtio, virtio_req};
use core::ptr;
use onyx_core::errno::{Errno, KResult};

const ATTR_DIRECTORY: u8 = 0x10;
const ATTR_LFN: u8 = 0x0F;
const FAT32_EOC: u32 = 0x0FFFFFF8;
const DIR_ENTRY_SIZE: usize = 32;
const ENTRIES_PER_SECTOR: usize = 512 / DIR_ENTRY_SIZE;

static mut G_DEV: usize = 0;
static mut G_SPC: u32 = 0;
static mut G_RESVD: u32 = 0;
static mut G_FAT_SZ: u32 = 0;
static mut G_ROOT_CLUSTER: u32 = 0;
static mut G_DATA_LBA: u32 = 0;
unsafe fn read_sec(lba: u64, buf: &mut [u8; 512]) -> KResult<()> {
    virtio_req::read(G_DEV, lba, buf.as_mut_ptr())
}

unsafe fn cluster_to_lba(cluster: u32) -> u64 {
    (G_DATA_LBA as u64) + ((cluster - 2) as u64) * (G_SPC as u64)
}

unsafe fn fat_entry(cluster: u32, buf: &mut [u8; 512]) -> u32 {
    let fat_off = cluster as u64 * 4;
    let fat_lba = G_RESVD as u64 + fat_off / 512;
    if read_sec(fat_lba, buf).is_err() {
        return FAT32_EOC;
    }
    let off = (fat_off % 512) as usize;
    u32::from_le_bytes([buf[off], buf[off + 1], buf[off + 2], buf[off + 3]]) & 0x0FFF_FFFF
}

unsafe fn is_eoc(v: u32) -> bool {
    v >= FAT32_EOC
}

/// Bug (fs MINOR #13): validate a cluster number before following it.
/// A corrupted FAT chain can contain cluster 0 (invalid), cluster 1
/// (reserved), or a cluster pointing past the data region. Without
/// this check, the FAT32 driver would loop forever following such
/// chains, hanging the kernel. We cap the cluster at a sane upper
/// bound derived from the partition size.
unsafe fn is_valid_cluster(cluster: u32) -> bool {
    // Cluster 0 and 1 are reserved in FAT32. Valid clusters start at 2.
    // Upper bound: we don't have the total cluster count handy here,
    // so we use a generous cap of 0x0FFFFFF7 (just below EOC range).
    // A corrupted entry pointing past the data region will fail the
    // read_cluster_sector call below anyway.
    cluster >= 2 && cluster < FAT32_EOC
}

unsafe fn read_cluster_sector(
    cluster: u32,
    sector_in_cluster: u32,
    buf: &mut [u8; 512],
) -> KResult<()> {
    let lba = cluster_to_lba(cluster) + sector_in_cluster as u64;
    read_sec(lba, buf)
}

fn fat32_name_8_3(name: &[u8]) -> [u8; 11] {
    let mut out = [0x20u8; 11]; // space-padded
    if name.is_empty() || name == b"." || name == b".." {
        return out;
    }
    let dot = name.iter().position(|&b| b == b'.');
    let (base, ext) = match dot {
        Some(i) => (&name[..i], &name[i + 1..]),
        None => (name, &[][..]),
    };
    for i in 0..base.len().min(8) {
        let b = base[i];
        out[i] = if b >= b'a' && b <= b'z' { b - 32 } else { b };
    }
    for i in 0..ext.len().min(3) {
        let b = ext[i];
        out[8 + i] = if b >= b'a' && b <= b'z' { b - 32 } else { b };
    }
    out
}

unsafe fn scan_dir_entries(
    dir_cluster: u32,
    needle: &[u8; 11],
    out_cluster: &mut u32,
    out_size: &mut u32,
    is_dir: &mut bool,
    buf: &mut [u8; 512],
) -> KResult<()> {
    let mut cluster = dir_cluster;
    if !is_valid_cluster(cluster) {
        return Err(Errno::NoEnt);
    }
    // Bug (fs MINOR #13): cap the number of cluster hops to prevent an
    // unbounded loop on a corrupted FAT chain (circular chain, or chain
    // that never reaches EOC). 65536 clusters × 8 sectors × 16 entries
    // = ~8 million directory entries — way more than any real FAT32 dir.
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
                    continue;
                }
                if buf[off] == 0 {
                    return Err(Errno::NoEnt);
                }
                if buf[off] == 0xE5 {
                    continue;
                }
                let mut entry = [0u8; 11];
                entry.copy_from_slice(&buf[off..off + 11]);
                if &entry == needle {
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
            }
        }
        let next = fat_entry(cluster, buf);
        if is_eoc(next) {
            return Err(Errno::NoEnt);
        }
        // Bug (fs MINOR #13): validate the next cluster before following.
        if !is_valid_cluster(next) {
            return Err(Errno::Io);
        }
        cluster = next;
    }
}

pub unsafe fn mount(dev: usize) -> KResult<()> {
    G_DEV = dev;
    let mut bpb = [0u8; 512];
    read_sec(0, &mut bpb)?;
    if bpb[510] != 0x55 || bpb[511] != 0xAA {
        return Err(Errno::Inval);
    }
    let bps = u16::from_le_bytes([bpb[11], bpb[12]]) as u32;
    if bps != 512 {
        return Err(Errno::Inval);
    }
    // Bug (fs MINOR #12): validate the BPB fields that we actually use.
    // A corrupted BPB with SPC=0 or reserved_sectors=0 or FAT_size=0
    // would cause division-by-zero or infinite loops later. Reject
    // obvious garbage values upfront.
    G_SPC = bpb[13] as u32;
    if G_SPC == 0 || G_SPC > 128 {
        return Err(Errno::Inval);
    }
    G_RESVD = u16::from_le_bytes([bpb[14], bpb[15]]) as u32;
    if G_RESVD == 0 {
        return Err(Errno::Inval);
    }
    G_FAT_SZ = u16::from_le_bytes([bpb[22], bpb[23]]) as u32;
    if G_FAT_SZ == 0 {
        G_FAT_SZ = u32::from_le_bytes([bpb[36], bpb[37], bpb[38], bpb[39]]);
    }
    if G_FAT_SZ == 0 {
        return Err(Errno::Inval);
    }
    G_ROOT_CLUSTER = u32::from_le_bytes([bpb[44], bpb[45], bpb[46], bpb[47]]);
    if G_ROOT_CLUSTER < 2 {
        return Err(Errno::Inval);
    }
    G_DATA_LBA = G_RESVD + 2 * G_FAT_SZ;
    Ok(())
}

// Helper: resolve one path component by scanning `dir_cluster`.
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

pub unsafe fn read(cluster: u32, buf: *mut u8, off: u32, len: u32) -> KResult<u32> {
    if len == 0 || cluster == 0 {
        return Ok(0);
    }
    let sector_size = 512u32;
    let cluster_bytes = G_SPC * sector_size;
    let end_byte = off as u64 + len as u64;

    let mut sec_buf = [0u8; 512];
    let mut cluster = cluster;
    // `cur_pos` is the file-space cursor (where we currently want to read
    // from). `cluster_base` is the file-space start of the cluster we are
    // currently walking. `total_copied` is the write cursor into `buf`.
    //
    // Bug #23 fix: the previous code computed `copy_off` from `start_byte`
    // for every cluster (using a single `start_byte` instead of an
    // advancing cursor) and wrote each cluster's bytes to `buf.add(0)`
    // via `copy_to = buf`. As a result, multi-cluster reads overwrote
    // the start of `buf` on every cluster and only the last cluster's
    // data survived in the output — anything bigger than one cluster
    // came back as garbage. The cursor triplet below fixes both issues.
    let mut cur_pos = off as u64;
    let mut total_copied: u32 = 0;
    let mut cluster_base: u64 = 0;

    loop {
        let cluster_end = cluster_base + cluster_bytes as u64;
        // Does this cluster contribute any bytes to the requested range?
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
            // Advance the file cursor by however many bytes we actually
            // copied from this cluster.
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
        // Bug (fs MINOR #13): validate the next cluster before following.
        if !is_valid_cluster(next) {
            return Ok(total_copied);
        }
        cluster = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fat32_name_simple() {
        let name = fat32_name_8_3(b"hello.txt");
        assert_eq!(&name[..8], b"HELLO   ");
        assert_eq!(&name[8..], b"TXT");
    }

    #[test]
    fn test_fat32_name_no_ext() {
        let name = fat32_name_8_3(b"foo");
        assert_eq!(&name[..8], b"FOO     ");
        assert_eq!(&name[8..], b"   ");
    }

    #[test]
    fn test_fat32_name_dot() {
        let name = fat32_name_8_3(b".");
        assert_eq!(name, [0x20u8; 11]);
    }

    #[test]
    fn test_fat32_name_dotdot() {
        let name = fat32_name_8_3(b"..");
        assert_eq!(name, [0x20u8; 11]);
    }

    #[test]
    fn test_fat32_name_empty() {
        let name = fat32_name_8_3(b"");
        assert_eq!(name, [0x20u8; 11]);
    }

    #[test]
    fn test_fat32_name_makefile() {
        let name = fat32_name_8_3(b"Makefile");
        assert_eq!(&name[..8], b"MAKEFILE");
        assert_eq!(&name[8..], b"   ");
    }

    #[test]
    fn test_fat32_name_long_ext() {
        let name = fat32_name_8_3(b"document.pdf");
        assert_eq!(&name[..8], b"DOCUMENT");
        assert_eq!(&name[8..], b"PDF");
    }

    #[test]
    fn test_fat32_name_uppercase() {
        let name = fat32_name_8_3(b"README.TXT");
        assert_eq!(&name[..8], b"README  ");
        assert_eq!(&name[8..], b"TXT");
    }

    #[test]
    fn test_fat32_is_eoc() {
        unsafe {
            assert!(is_eoc(0x0FFFFFF8));
            assert!(is_eoc(0x0FFFFFF9));
            assert!(is_eoc(0x0FFFFFFF));
            assert!(!is_eoc(0x0FFFFFF7));
            assert!(!is_eoc(0x0FFFFFF6));
            assert!(!is_eoc(2));
            assert!(!is_eoc(0));
        }
    }

    #[test]
    fn test_fat32_valid_cluster() {
        unsafe {
            assert!(is_valid_cluster(2));
            assert!(is_valid_cluster(100));
            assert!(is_valid_cluster(0x0FFFFFF6));
            assert!(!is_valid_cluster(0));
            assert!(!is_valid_cluster(1));
            assert!(!is_valid_cluster(FAT32_EOC));
            assert!(!is_valid_cluster(0x0FFFFFF8));
        }
    }
}

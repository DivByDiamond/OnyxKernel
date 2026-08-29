use super::{
    ATTR_DIRECTORY, ATTR_LFN, DIR_ENTRY_SIZE, ENTRIES_PER_SECTOR, FAT32_EOC, G_DATA_LBA, G_DEV,
    G_FAT_SZ, G_NUM_FATS, G_RESVD, G_ROOT_CLUSTER, G_SPC, cluster_to_lba, fat_entry, is_eoc,
    read_sec,
};
use onyx_core::errno::{Errno, KResult};

/// True if `cluster` is a valid data-cluster number (2..EOC).
///
/// # Safety
///
/// Pure predicate; unsafe only to match module convention. No raw access.
pub(crate) unsafe fn is_valid_cluster(cluster: u32) -> bool {
    (2..FAT32_EOC).contains(&cluster)
}

/// Read one 512-byte sector from within `cluster`'s data area.
///
/// # Safety
///
/// Caller must not invoke FAT32 I/O concurrently from multiple harts; mount()
/// must have initialized the globals, and `cluster` must be a valid data
/// cluster (callers check is_valid_cluster) so cluster_to_lba cannot
/// underflow. `buf` is a caller-owned 512-byte buffer.
pub(crate) unsafe fn read_cluster_sector(
    cluster: u32,
    sector_in_cluster: u32,
    buf: &mut [u8; 512],
) -> KResult<()> {
    // SAFETY: see # Safety; buf is a valid 512-byte buffer, the LBA
    // arithmetic cannot underflow for a validated cluster.
    unsafe {
        let lba = cluster_to_lba(cluster) + sector_in_cluster as u64;
        read_sec(lba, buf)
    }
}

pub(crate) fn fat32_name_8_3(name: &[u8]) -> [u8; 11] {
    let mut out = [0x20u8; 11];
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
        out[i] = if b.is_ascii_lowercase() { b - 32 } else { b };
    }
    for i in 0..ext.len().min(3) {
        let b = ext[i];
        out[8 + i] = if b.is_ascii_lowercase() { b - 32 } else { b };
    }
    out
}

/// Walk a directory cluster chain looking for the 8.3 name `needle`.
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
    needle: &[u8; 11],
    out_cluster: &mut u32,
    out_size: &mut u32,
    is_dir: &mut bool,
    buf: &mut [u8; 512],
) -> KResult<()> {
    // SAFETY: see # Safety; entry offsets are ei*32 with ei < 16, so all
    // buffer reads stay within the 512-byte sector.
    unsafe {
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
            if !is_valid_cluster(next) {
                return Err(Errno::Io);
            }
            cluster = next;
        }
    }
}

/// Classify a volume by its data-cluster count (Microsoft FAT spec):
/// < 4085 → FAT12, < 65525 → FAT16, otherwise FAT32. This driver only
/// implements FAT32, so anything below is rejected at mount time.
pub(crate) fn fat_type_for_clusters(count_of_clusters: u64) -> &'static str {
    if count_of_clusters < 4085 {
        "FAT12"
    } else if count_of_clusters < 65525 {
        "FAT16"
    } else {
        "FAT32"
    }
}

/// # Safety
///
/// Must be called once at boot before secondary harts are released (see
/// `release_secondary_harts()` in srv/main/init.rs): it writes the module
/// globals without a lock. All BPB fields are validated in-line and the
/// volume is required to be FAT32 (not FAT12/16).
pub unsafe fn mount(dev: usize) -> KResult<()> {
    // SAFETY: single-threaded boot init is the exclusion guarantee for the
    // module globals written here.
    unsafe {
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
        G_SPC = bpb[13] as u32;
        if G_SPC == 0 || G_SPC > 128 {
            return Err(Errno::Inval);
        }
        G_RESVD = u16::from_le_bytes([bpb[14], bpb[15]]) as u32;
        if G_RESVD == 0 {
            return Err(Errno::Inval);
        }
        // BPB_NumFATs: all FAT writes are mirrored across this many
        // copies (write::fat). The spec uses 1..=2; anything else is
        // treated as a corrupt BPB rather than mirrored wrongly.
        G_NUM_FATS = bpb[16] as u32;
        if !(1..=2).contains(&G_NUM_FATS) {
            crate::kwrn!("fat32", "mount: BPB_NumFATs out of range, refusing");
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
        // Filesystem-type check: derive the data-cluster count from the
        // BPB and require the FAT32 range. Mounting FAT12/16 here would
        // misparse cluster chains and could free arbitrary clusters on
        // the write paths.
        let root_entries = u16::from_le_bytes([bpb[17], bpb[18]]) as u64;
        let root_secs = root_entries.div_ceil(512 / 32);
        let tot16 = u16::from_le_bytes([bpb[19], bpb[20]]) as u64;
        let tot32 = u32::from_le_bytes([bpb[32], bpb[33], bpb[34], bpb[35]]) as u64;
        let total_secs = if tot16 != 0 { tot16 } else { tot32 };
        let fat_secs = G_NUM_FATS as u64 * G_FAT_SZ as u64;
        let data_secs = total_secs.saturating_sub(G_RESVD as u64 + fat_secs + root_secs);
        let count_of_clusters = data_secs / G_SPC as u64;
        if fat_type_for_clusters(count_of_clusters) != "FAT32" {
            crate::kwrn!("fat32", "mount: not a FAT32 volume, refusing");
            return Err(Errno::Inval);
        }
        G_DATA_LBA = G_RESVD + G_NUM_FATS * G_FAT_SZ;
        Ok(())
    }
}

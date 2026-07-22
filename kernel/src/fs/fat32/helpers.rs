use super::{
    cluster_to_lba, fat_entry, is_eoc, read_sec, ATTR_DIRECTORY, ATTR_LFN, DIR_ENTRY_SIZE,
    ENTRIES_PER_SECTOR, FAT32_EOC, G_DATA_LBA, G_DEV, G_FAT_SZ, G_RESVD, G_ROOT_CLUSTER, G_SPC,
};
use onyx_core::errno::{Errno, KResult};

pub(crate) unsafe fn is_valid_cluster(cluster: u32) -> bool {
    cluster >= 2 && cluster < FAT32_EOC
}

pub(crate) unsafe fn read_cluster_sector(
    cluster: u32,
    sector_in_cluster: u32,
    buf: &mut [u8; 512],
) -> KResult<()> {
    let lba = cluster_to_lba(cluster) + sector_in_cluster as u64;
    read_sec(lba, buf)
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
        out[i] = if b >= b'a' && b <= b'z' { b - 32 } else { b };
    }
    for i in 0..ext.len().min(3) {
        let b = ext[i];
        out[8 + i] = if b >= b'a' && b <= b'z' { b - 32 } else { b };
    }
    out
}

pub(crate) unsafe fn scan_dir_entries(
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

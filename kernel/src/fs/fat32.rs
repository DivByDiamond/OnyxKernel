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
    if cluster == 0 {
        return Err(Errno::NoEnt);
    }
    loop {
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
    G_RESVD = u16::from_le_bytes([bpb[14], bpb[15]]) as u32;
    G_FAT_SZ = u16::from_le_bytes([bpb[22], bpb[23]]) as u32;
    if G_FAT_SZ == 0 {
        G_FAT_SZ = u32::from_le_bytes([bpb[36], bpb[37], bpb[38], bpb[39]]);
    }
    G_ROOT_CLUSTER = u32::from_le_bytes([bpb[44], bpb[45], bpb[46], bpb[47]]);
    G_DATA_LBA = G_RESVD + 2 * G_FAT_SZ;
    Ok(())
}

pub unsafe fn lookup(path: &[u8], out_cluster: &mut u32, out_size: &mut u32) -> KResult<()> {
    let needle = fat32_name_8_3(path);
    let mut is_dir = false;
    let mut buf = [0u8; 512];
    scan_dir_entries(
        G_ROOT_CLUSTER,
        &needle,
        out_cluster,
        out_size,
        &mut is_dir,
        &mut buf,
    )
}

pub unsafe fn read(cluster: u32, buf: *mut u8, off: u32, len: u32) -> KResult<u32> {
    if len == 0 || cluster == 0 {
        return Ok(0);
    }
    let sector_size = 512u32;
    let cluster_bytes = G_SPC * sector_size;
    let start_byte = off as u64;
    let end_byte = (off as u64 + len as u64).min(u32::MAX as u64);

    let mut sec_buf = [0u8; 512];
    let mut cluster = cluster;
    let mut skipped = 0u64;
    loop {
        let csize = cluster_bytes as u64;
        let cstart = skipped;
        let cend = skipped + csize;
        if start_byte < cend {
            let rel_start = (start_byte - cstart) as u32;
            let rel_end = (end_byte - cstart).min(csize) as u32;
            let to_copy = rel_end - rel_start;
            if to_copy > 0 {
                let copy_off = rel_start;
                let copy_to = buf;
                let mut remain = to_copy;
                let mut copy_pos = 0u32;
                while remain > 0 {
                    let si = copy_off / sector_size;
                    let sec_off = copy_off % sector_size;
                    let in_sec = sector_size - sec_off;
                    let chunk = remain.min(in_sec);
                    read_cluster_sector(cluster, si, &mut sec_buf)?;
                    ptr::copy_nonoverlapping(
                        sec_buf.as_ptr().add(sec_off as usize),
                        copy_to.add(copy_pos as usize),
                        chunk as usize,
                    );
                    copy_pos += chunk;
                    remain -= chunk;
                }
            }
            if end_byte <= cend {
                return Ok(to_copy);
            }
        }
        skipped += csize;
        let next = fat_entry(cluster, &mut sec_buf);
        if is_eoc(next) {
            return Ok((end_byte - start_byte) as u32);
        }
        cluster = next;
    }
}

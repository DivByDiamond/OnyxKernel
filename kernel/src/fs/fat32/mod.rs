//! FAT32 read-only driver.
use crate::drivers::virtio_req;
use onyx_core::errno::{Errno, KResult};

const ATTR_DIRECTORY: u8 = 0x10;
const ATTR_LFN: u8 = 0x0F;
const FAT32_EOC: u32 = 0x0FFFFFF8;
const DIR_ENTRY_SIZE: usize = 32;
const ENTRIES_PER_SECTOR: usize = 512 / DIR_ENTRY_SIZE;

pub(crate) static mut G_DEV: usize = 0;
pub(crate) static mut G_SPC: u32 = 0;
pub(crate) static mut G_RESVD: u32 = 0;
pub(crate) static mut G_FAT_SZ: u32 = 0;
pub(crate) static mut G_ROOT_CLUSTER: u32 = 0;
pub(crate) static mut G_DATA_LBA: u32 = 0;

pub(crate) unsafe fn read_sec(lba: u64, buf: &mut [u8; 512]) -> KResult<()> {
    virtio_req::read(G_DEV, lba, buf.as_mut_ptr())
}

pub(crate) unsafe fn cluster_to_lba(cluster: u32) -> u64 {
    (G_DATA_LBA as u64) + ((cluster - 2) as u64) * (G_SPC as u64)
}

pub(crate) unsafe fn fat_entry(cluster: u32, buf: &mut [u8; 512]) -> u32 {
    let fat_off = cluster as u64 * 4;
    let fat_lba = G_RESVD as u64 + fat_off / 512;
    if read_sec(fat_lba, buf).is_err() {
        return FAT32_EOC;
    }
    let off = (fat_off % 512) as usize;
    u32::from_le_bytes([buf[off], buf[off + 1], buf[off + 2], buf[off + 3]]) & 0x0FFF_FFFF
}

pub(crate) unsafe fn is_eoc(v: u32) -> bool {
    v >= FAT32_EOC
}

mod helpers;
mod dir;

pub(crate) use helpers::*;
pub use dir::*;

#[cfg(test)]
mod tests;

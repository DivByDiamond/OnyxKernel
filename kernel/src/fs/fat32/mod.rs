//! FAT32 read-only driver.
use crate::drivers::virtio_req;
use onyx_core::errno::KResult;

const ATTR_DIRECTORY: u8 = 0x10;
const ATTR_LFN: u8 = 0x0F;
const FAT32_EOC: u32 = 0x0FFFFFF8;
const DIR_ENTRY_SIZE: usize = 32;
const ENTRIES_PER_SECTOR: usize = 512 / DIR_ENTRY_SIZE;

pub(crate) static mut G_DEV: usize = 0;
pub(crate) static mut G_SPC: u32 = 0;
pub(crate) static mut G_RESVD: u32 = 0;
pub(crate) static mut G_FAT_SZ: u32 = 0;
/// BPB_NumFATs — number of FAT copies on the volume. All FAT writes are
/// mirrored to every copy (see write::write_fat_entry).
pub(crate) static mut G_NUM_FATS: u32 = 0;
pub(crate) static mut G_ROOT_CLUSTER: u32 = 0;
pub(crate) static mut G_DATA_LBA: u32 = 0;

/// # Safety
///
/// Caller must not invoke FAT32 I/O concurrently from multiple harts (the
/// module globals are unsynchronized); `mount()` must have initialized
/// G_DEV. `buf` is a caller-owned 512-byte buffer.
pub(crate) unsafe fn read_sec(lba: u64, buf: &mut [u8; 512]) -> KResult<()> {
    // SAFETY: buf is a valid, writable 512-byte buffer; G_DEV was set by
    // mount() before any FAT32 I/O (single-threaded boot init).
    unsafe { virtio_req::read(G_DEV, lba, buf.as_mut_ptr()) }
}

/// Convert a cluster number to its absolute LBA. Requires mount() to have
/// initialized G_DATA_LBA and G_SPC.
///
/// # Safety
///
/// `cluster` must be >= 2 and a valid data cluster (see is_valid_cluster);
/// callers pass clusters validated against the FAT, so the subtract cannot
/// underflow. Reads only the mount-initialized globals.
pub(crate) unsafe fn cluster_to_lba(cluster: u32) -> u64 {
    // SAFETY: caller contract above guarantees cluster >= 2 (no underflow);
    // only reads mount-initialized globals.
    unsafe { (G_DATA_LBA as u64) + ((cluster - 2) as u64) * (G_SPC as u64) }
}

/// Read the 4-byte FAT entry for `cluster` from FAT copy 0. Returns EOC on
/// read failure so callers treat it as end-of-chain.
///
/// # Safety
///
/// Caller must not invoke FAT32 I/O concurrently from multiple harts;
/// `cluster` must be a valid FAT index (callers check is_valid_cluster
/// first). `buf` is a caller-owned 512-byte scratch buffer.
pub(crate) unsafe fn fat_entry(cluster: u32, buf: &mut [u8; 512]) -> u32 {
    // SAFETY: buf is a valid 512-byte buffer; fat_off is a multiple of 4 and
    // 512 % 4 == 0, so off = fat_off % 512 is <= 508 and off+3 stays in
    // bounds of the sector read into buf.
    unsafe {
        let fat_off = cluster as u64 * 4;
        let fat_lba = G_RESVD as u64 + fat_off / 512;
        if read_sec(fat_lba, buf).is_err() {
            return FAT32_EOC;
        }
        let off = (fat_off % 512) as usize;
        u32::from_le_bytes([buf[off], buf[off + 1], buf[off + 2], buf[off + 3]]) & 0x0FFF_FFFF
    }
}

/// True if `v` is an end-of-chain marker per the FAT32 spec.
///
/// # Safety
///
/// Pure predicate over a u32; unsafe only to match the module's convention
/// of requiring the mount-initialized context. No raw dereferences.
pub(crate) unsafe fn is_eoc(v: u32) -> bool {
    v >= FAT32_EOC
}

mod dir;
mod helpers;
mod write;

pub use dir::*;
pub(crate) use helpers::*;
pub use write::*;

#[cfg(test)]
mod tests;

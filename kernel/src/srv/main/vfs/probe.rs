//! Filesystem superblock probes that do not mutate the active mount state.

use crate::drivers::virtio::virtio_req;
use crate::fs::fat32::fat_type_for_clusters;
use onyx_core::formats::{ONYFS_BLOCK_SIZE, ONYFS_MAGIC, ONYFS_MAGIC_V1, OnyfsSuper};

/// Probe for OnyxFS superblock without mutating the global G_DEV/G_SB.
/// Reads 8 sectors (4096 bytes) starting at `lba` and checks magic/block_size.
///
/// # Safety
///
/// Boot-time virtio-blk probe. `dev` must name a registered virtio-blk device
/// and `lba` must be within its capacity; callers discover candidates from
/// the block-device count returned by virtio initialization.
pub(super) unsafe fn probe_onyxfs(dev: usize, lba: u32) -> bool {
    // SAFETY: boot-time single-hart device probe; `buf` is the exact 4 KiB
    // request length and the virtio index is caller-validated.
    unsafe {
        let mut buf = [0u8; 4096];
        if virtio_req::read_multi(dev, lba as u64, 8, buf.as_mut_ptr()).is_err() {
            return false;
        }
        if let Some(sb) = OnyfsSuper::from_bytes(&buf) {
            if sb.block_size != ONYFS_BLOCK_SIZE as u32 {
                return false;
            }
            sb.magic == ONYFS_MAGIC || sb.magic == ONYFS_MAGIC_V1
        } else {
            false
        }
    }
}

/// Probe for FAT32 without mutating globals: check 0x55AA and FAT type.
/// Minimal check matching `fat32::helpers::mount` requirements.
///
/// # Safety
///
/// Boot-time virtio-blk probe. `dev` must name a registered virtio-blk device.
pub(super) unsafe fn probe_fat32(dev: usize) -> bool {
    // SAFETY: boot-time single-hart device probe; `dev` is caller-validated
    // and `bpb` is a full 512-byte sector read before being parsed.
    unsafe {
        let mut bpb = [0u8; 512];
        if virtio_req::read(dev, 0, bpb.as_mut_ptr()).is_err() {
            return false;
        }
        if bpb[510] != 0x55 || bpb[511] != 0xAA {
            return false;
        }
        let bps = u16::from_le_bytes([bpb[11], bpb[12]]) as u32;
        if bps != 512 {
            return false;
        }
        let spc = bpb[13] as u32;
        if spc == 0 || spc > 128 {
            return false;
        }
        let resvd = u16::from_le_bytes([bpb[14], bpb[15]]) as u32;
        if resvd == 0 {
            return false;
        }
        let num_fats = bpb[16] as u32;
        if !(1..=2).contains(&num_fats) {
            return false;
        }
        let fatsz16 = u16::from_le_bytes([bpb[22], bpb[23]]) as u32;
        let fatsz32 = u32::from_le_bytes([bpb[36], bpb[37], bpb[38], bpb[39]]);
        let fatsz = if fatsz16 != 0 { fatsz16 } else { fatsz32 };
        if fatsz == 0 {
            return false;
        }
        let root_entries = u16::from_le_bytes([bpb[17], bpb[18]]) as u64;
        let root_secs = root_entries.div_ceil(512 / 32);
        let tot16 = u16::from_le_bytes([bpb[19], bpb[20]]) as u64;
        let tot32 = u32::from_le_bytes([bpb[32], bpb[33], bpb[34], bpb[35]]) as u64;
        let total_secs = if tot16 != 0 { tot16 } else { tot32 };
        let fat_secs = num_fats as u64 * fatsz as u64;
        let data_secs = total_secs.saturating_sub(resvd as u64 + fat_secs + root_secs);
        let count_of_clusters = data_secs / spc as u64;
        fat_type_for_clusters(count_of_clusters) == "FAT32"
    }
}

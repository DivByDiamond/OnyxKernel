use crate::parser::{le32, le64};
use alloc::vec::Vec;

// OnyxFS v2 — timestamps (ext4-style), snapshots, indirect blocks
// ════════════════════════════════════════════════════════════════════════════

pub const ONYFS_MAGIC: u32 = 0x32594E4F; // 'ONY2' LE — v2 magic
pub const ONYFS_MAGIC_V1: u32 = 0x31594E4F; // 'ONY1' LE — v1 compat
pub const ONYFS_VERSION: u32 = 2;
pub const ONYFS_BLOCK_SIZE: usize = 4096;
pub const ONYFS_NAME_MAX: usize = 32;
pub const ONYFS_DIRECT_BLKS: usize = 10;
pub const ONYFS_INDIRECT_BLKS: usize = 1; // single indirect
pub const ONYFS_ROOT_INO: u32 = 1;
pub const ONYFS_DT_REG: u32 = 0o100755;
pub const ONYFS_DT_DIR: u32 = 0o040755;
pub const ONYFS_DT_LNK: u32 = 0o120755; // symlink
pub const ONYFS_DT_SNAPSHOT: u32 = 0o140755; // snapshot marker

pub const ONYFS_FEAT_TIMESTAMPS: u32 = 0x1;
pub const ONYFS_FEAT_SNAPSHOTS: u32 = 0x2;
pub const ONYFS_FEAT_COMPRESSION: u32 = 0x4;
pub const ONYFS_FEAT_JOURNAL: u32 = 0x8;

/// OnyxFS v2 superblock — 128 bytes (expanded from 64).
/// Added: snapshot area, journal area, feature flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OnyfsSuper {
    pub magic: u32,
    pub version: u32,
    pub block_size: u32,
    pub total_blocks: u32,
    pub inode_count: u32,
    pub inode_table_start: u32,
    pub data_bitmap_start: u32,
    pub data_blocks_start: u32,
    pub root_inode: u32,
    // v2 additions:
    pub snapshot_area_start: u32, // block where snapshot metadata lives
    pub snapshot_count: u32,      // number of snapshots
    pub journal_start: u32,       // journal area for crash recovery
    pub journal_size: u32,        // journal size in blocks
    pub feature_flags: u32,       // FEATURE_TIMESTAMPS | FEATURE_SNAPSHOTS | FEATURE_COMPRESSION
    pub creation_time: u64,       // filesystem creation time (nanoseconds since epoch)
    pub last_mount_time: u64,     // last mount time
    pub reserved: [u32; 10],      // future expansion
}

/// Hard cap for grow-on-mount: never grow a filesystem beyond 1 GiB even if
/// the backing device is larger. 1 GiB / 4096 = 262144 blocks.
pub const ONYFS_MAX_TOTAL_BLOCKS: u32 = (1024 * 1024 * 1024 / ONYFS_BLOCK_SIZE) as u32;
/// The v2 data-area bitmap is exactly ONE block written by mkimage at block
/// index 2, so at most `4096 * 8` data blocks can ever be addressed without a
/// format change. This is the real ceiling for grow-on-mount (~128 MiB of
/// data area on top of the fixed metadata prefix).
pub const ONYFS_BITMAP_CAPACITY_BLOCKS: u32 = (ONYFS_BLOCK_SIZE * 8) as u32;

/// Maximum total_blocks reachable by growing a filesystem whose data area
/// starts at `data_blocks_start`: bounded both by the single-block bitmap
/// capacity and the 1 GiB sanity cap.
#[inline]
pub fn onyxfs_growth_limit(data_blocks_start: u32) -> u64 {
    let bitmap_cap = data_blocks_start as u64 + ONYFS_BITMAP_CAPACITY_BLOCKS as u64;
    bitmap_cap.min(ONYFS_MAX_TOTAL_BLOCKS as u64)
}

/// Grow-on-mount target: given the current `total_blocks`, the start of the
/// data area, and the device size in FS blocks, return the new `total_blocks`
/// to use, or `None` if the device is not larger than the filesystem.
///
/// The result never exceeds the growth limit above. No bitmap rewrite is
/// needed: mkimage leaves every data-bitmap bit beyond the built image at
/// zero (= free), so newly exposed blocks are immediately allocatable.
pub fn onyxfs_growth_target(
    cur_total_blocks: u32,
    data_blocks_start: u32,
    device_blocks: u64,
) -> Option<u32> {
    if device_blocks <= cur_total_blocks as u64 {
        return None;
    }
    let new_total = device_blocks.min(onyxfs_growth_limit(data_blocks_start));
    let new_total = u32::try_from(new_total).ok()?;
    if new_total > cur_total_blocks {
        Some(new_total)
    } else {
        None
    }
}

impl OnyfsSuper {
    pub const SIZE: usize = 128;

    pub fn from_bytes(buf: &[u8]) -> Option<Self> {
        if buf.len() < Self::SIZE {
            return None;
        }
        let magic = le32(&buf[0..4]);
        if magic != ONYFS_MAGIC && magic != ONYFS_MAGIC_V1 {
            return None;
        }
        Some(Self {
            magic,
            version: le32(&buf[4..8]),
            block_size: le32(&buf[8..12]),
            total_blocks: le32(&buf[12..16]),
            inode_count: le32(&buf[16..20]),
            inode_table_start: le32(&buf[20..24]),
            data_bitmap_start: le32(&buf[24..28]),
            data_blocks_start: le32(&buf[28..32]),
            root_inode: le32(&buf[32..36]),
            snapshot_area_start: le32(&buf[36..40]),
            snapshot_count: le32(&buf[40..44]),
            journal_start: le32(&buf[44..48]),
            journal_size: le32(&buf[48..52]),
            feature_flags: le32(&buf[52..56]),
            creation_time: le64(&buf[56..64]),
            last_mount_time: le64(&buf[64..72]),
            reserved: [0; 10],
        })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut b = alloc::vec![0u8; Self::SIZE];
        b[0..4].copy_from_slice(&self.magic.to_le_bytes());
        b[4..8].copy_from_slice(&self.version.to_le_bytes());
        b[8..12].copy_from_slice(&self.block_size.to_le_bytes());
        b[12..16].copy_from_slice(&self.total_blocks.to_le_bytes());
        b[16..20].copy_from_slice(&self.inode_count.to_le_bytes());
        b[20..24].copy_from_slice(&self.inode_table_start.to_le_bytes());
        b[24..28].copy_from_slice(&self.data_bitmap_start.to_le_bytes());
        b[28..32].copy_from_slice(&self.data_blocks_start.to_le_bytes());
        b[32..36].copy_from_slice(&self.root_inode.to_le_bytes());
        b[36..40].copy_from_slice(&self.snapshot_area_start.to_le_bytes());
        b[40..44].copy_from_slice(&self.snapshot_count.to_le_bytes());
        b[44..48].copy_from_slice(&self.journal_start.to_le_bytes());
        b[48..52].copy_from_slice(&self.journal_size.to_le_bytes());
        b[52..56].copy_from_slice(&self.feature_flags.to_le_bytes());
        b[56..64].copy_from_slice(&self.creation_time.to_le_bytes());
        b[64..72].copy_from_slice(&self.last_mount_time.to_le_bytes());
        b
    }
}

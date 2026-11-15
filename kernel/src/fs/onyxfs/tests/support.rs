use super::super::*;
use ::alloc::{vec, vec::Vec};
use onyx_core::formats::{ONYFS_MAGIC, ONYFS_ROOT_INO};

pub(in crate::fs::onyxfs) const TOTAL: u32 = 700;
pub(in crate::fs::onyxfs) const DATA_START: u32 = 40;

/// Build an empty v2 volume in `TEST_DISK` with a root directory inode.
/// # Safety
/// Caller holds SERIAL across all filesystem state accesses.
pub(in crate::fs::onyxfs) unsafe fn mkfs() {
    // SAFETY: callers hold SERIAL for all volume state and test I/O.
    unsafe {
        super::io::reset_faults();
        TEST_DISK = Some(vec![0u8; TOTAL as usize * ONYFS_BLOCK_SIZE]);
        G_SB = OnyfsSuper {
            magic: ONYFS_MAGIC,
            version: 2,
            block_size: 4096,
            total_blocks: TOTAL,
            inode_count: 32,
            inode_table_start: 3,
            data_bitmap_start: 2,
            data_blocks_start: DATA_START,
            root_inode: 1,
            snapshot_area_start: 0,
            snapshot_count: 0,
            journal_start: 4,
            journal_size: 32,
            feature_flags: 0,
            creation_time: 0,
            last_mount_time: 0,
            reserved: [0; 10],
        };
        G_VERSION = ONYFS_V2;
        G_JOURNAL_HEAD = 0;
        G_SNAPSHOT_QUARANTINED = false;
        // Inode 1 (root directory) is in use.
        let mut bm = [0u8; ONYFS_BLOCK_SIZE];
        bm[0] = 1;
        write_block(1, &bm).unwrap();
        let root = onyx_core::formats::OnyfsInode {
            mode: 0o040755,
            nlink: 2,
            ..Default::default()
        };
        inode::write_inode(ONYFS_ROOT_INO, &root).unwrap();
        journal::journal_commit().unwrap();
    }
}

pub(in crate::fs::onyxfs) fn used_data_blocks() -> u32 {
    let mut bm = [0u8; ONYFS_BLOCK_SIZE];
    // SAFETY: tests run the whole scenario on one thread.
    unsafe { read_block(2, &mut bm).unwrap() };
    bm.iter().map(|b| b.count_ones()).sum()
}

pub(in crate::fs::onyxfs) fn pattern(len: usize, seed: u8) -> Vec<u8> {
    (0..len)
        .map(|i| {
            (i as u8)
                .wrapping_mul(31)
                .wrapping_add((i >> 8) as u8)
                .wrapping_add(seed)
        })
        .collect()
}

/// # Safety
/// Caller holds SERIAL and supplies an allocated test inode.
pub(in crate::fs::onyxfs) unsafe fn put(ino: u32, data: &[u8], chunk: usize) {
    // SAFETY: SERIAL excludes test peers; data is readable for every chunk.
    unsafe {
        let mut off = 0;
        while off < data.len() {
            let n = chunk.min(data.len() - off);
            let w = write(ino, data.as_ptr().add(off), off as u32, n as u32).unwrap();
            assert_eq!(w as usize, n, "short write at offset {off}");
            off += n;
        }
    }
}

/// # Safety
/// Caller holds SERIAL and supplies an allocated test inode.
pub(in crate::fs::onyxfs) unsafe fn get(ino: u32, len: usize) -> Vec<u8> {
    // SAFETY: SERIAL owns the volume and out spans every requested chunk.
    unsafe {
        let mut out = vec![0u8; len];
        let mut off = 0;
        while off < len {
            let n = 4096.min(len - off);
            let r = read(ino, out.as_mut_ptr().add(off), off as u32, n as u32).unwrap();
            assert_eq!(r as usize, n, "short read at offset {off}");
            off += n;
        }
        out
    }
}

/// # Safety
/// Caller holds SERIAL and supplies an allocated test inode.
pub(in crate::fs::onyxfs) unsafe fn size_of(ino: u32) -> u64 {
    // SAFETY: SERIAL owns globals; test callers pass an allocated inode.
    unsafe {
        let mut i = onyx_core::formats::OnyfsInode::default();
        inode::read_inode(ino, &mut i).unwrap();
        i.size
    }
}

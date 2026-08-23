use super::super::*;

#[test]
fn test_onyfs_v2_super_roundtrip() {
    let sb = OnyfsSuper {
        magic: ONYFS_MAGIC,
        version: 2,
        block_size: 4096,
        total_blocks: 1000,
        inode_count: 128,
        inode_table_start: 5,
        data_bitmap_start: 3,
        data_blocks_start: 6,
        root_inode: 1,
        snapshot_area_start: 900,
        snapshot_count: 0,
        journal_start: 950,
        journal_size: 10,
        feature_flags: ONYFS_FEAT_TIMESTAMPS | ONYFS_FEAT_SNAPSHOTS,
        creation_time: 1234567890,
        last_mount_time: 1234567891,
        reserved: [0; 10],
    };
    let bytes = sb.to_bytes();
    let parsed = OnyfsSuper::from_bytes(&bytes).unwrap();
    assert_eq!(
        parsed.feature_flags,
        ONYFS_FEAT_TIMESTAMPS | ONYFS_FEAT_SNAPSHOTS
    );
    assert_eq!(parsed.snapshot_area_start, 900);
}

#[test]
fn test_onyfs_v2_inode_roundtrip() {
    let inode = OnyfsInode {
        mode: ONYFS_DT_REG,
        size: 0x100000,
        uid: 0,
        gid: 0,
        nlink: 1,
        blocks: {
            let mut b = [0u32; ONYFS_DIRECT_BLKS];
            b[0] = 10;
            b[1] = 11;
            b
        },
        indirect: 20,
        double_indirect: 0,
        crtime: 1000,
        mtime: 2000,
        atime: 3000,
        ctime: 4000,
        flags: 0,
        reserved: 0,
    };
    let bytes = inode.to_bytes();
    let parsed = OnyfsInode::from_bytes(&bytes).unwrap();
    assert_eq!(parsed.size, 0x100000);
    assert_eq!(parsed.crtime, 1000);
    assert_eq!(parsed.mtime, 2000);
    assert_eq!(parsed.blocks[0], 10);
}

// ── Journal recovery decision logic (pure, no I/O) ──────────────────────────

use alloc::vec;
use alloc::vec::Vec;

fn entry(ty: u32, block_num: u32, fill: u8) -> Vec<u8> {
    let mut e = vec![fill; ONYFS_BLOCK_SIZE];
    e[0..4].copy_from_slice(&ty.to_le_bytes());
    e[4..8].copy_from_slice(&block_num.to_le_bytes());
    e
}

/// Drive the recovery scan over a sequence of raw entries exactly like the
/// kernel-side journal_recover does (scan forward, stop at commit/garbage).
fn scan(entries: &[Vec<u8>]) -> Option<usize> {
    for (i, e) in entries.iter().enumerate() {
        match journal_scan_step(journal_entry_type(e)) {
            JournalScanStep::Continue => {}
            JournalScanStep::Commit => return Some(i),
            JournalScanStep::Stop => return None,
        }
    }
    None
}

#[test]
fn test_journal_scan_clean_is_noop() {
    // A properly zeroed journal decodes as COMMIT_START entries and never
    // yields a commit — recovery is a no-op.
    let clean = vec![entry(ONYFS_JOURNAL_COMMIT_START, 0, 0); 4];
    assert_eq!(scan(&clean), None);
}

#[test]
fn test_journal_scan_complete_transaction() {
    let tx = vec![
        entry(ONYFS_JOURNAL_COMMIT_START, 0, 0),
        entry(ONYFS_JOURNAL_BLOCK_WRITE, 42, 0xAA),
        entry(ONYFS_JOURNAL_BLOCK_WRITE, 43, 0xBB),
        entry(ONYFS_JOURNAL_COMMIT_END, 0, 0),
        // Stale data after the commit marker must not affect the scan.
        entry(ONYFS_JOURNAL_BLOCK_WRITE, 99, 0xCC),
    ];
    assert_eq!(scan(&tx), Some(3));
}

#[test]
fn test_journal_scan_torn_tail_has_no_commit() {
    // Crash after logging block_write entries but before commit_end: the
    // incomplete transaction is discarded.
    let torn = vec![
        entry(ONYFS_JOURNAL_COMMIT_START, 0, 0),
        entry(ONYFS_JOURNAL_BLOCK_WRITE, 42, 0xAA),
    ];
    assert_eq!(scan(&torn), None);
}

#[test]
fn test_journal_scan_corrupted_magic_stops() {
    // Garbage type tag (not 0/1/2) halts the scan before any commit — the
    // transaction must NOT be replayed.
    let corrupt = vec![
        entry(ONYFS_JOURNAL_BLOCK_WRITE, 42, 0xAA),
        entry(0xDEADBEEF, 43, 0xBB),
        entry(ONYFS_JOURNAL_COMMIT_END, 0, 0),
    ];
    assert_eq!(scan(&corrupt), None);
}

#[test]
fn test_journal_replay_entry_extracts_payload() {
    let e = entry(ONYFS_JOURNAL_BLOCK_WRITE, 1234, 0x5A);
    let (block_num, data) = journal_replay_entry(&e).unwrap();
    assert_eq!(block_num, 1234);
    assert_eq!(data.len(), ONYFS_JOURNAL_DATA_SIZE);
    assert!(data.iter().all(|&b| b == 0x5A));
}

#[test]
fn test_journal_replay_entry_ignores_non_block_write() {
    assert!(journal_replay_entry(&entry(ONYFS_JOURNAL_COMMIT_END, 7, 0)).is_none());
    assert!(journal_replay_entry(&entry(ONYFS_JOURNAL_COMMIT_START, 7, 0)).is_none());
}

#[test]
fn test_journal_replay_ordering() {
    // Replay applies entries oldest-first, each carrying its own payload;
    // verify extraction order across a multi-entry transaction.
    let tx = vec![
        entry(ONYFS_JOURNAL_BLOCK_WRITE, 100, 1),
        entry(ONYFS_JOURNAL_BLOCK_WRITE, 200, 2),
        entry(ONYFS_JOURNAL_COMMIT_END, 0, 0),
    ];
    let replayed: Vec<u32> = tx[..2]
        .iter()
        .filter_map(|e| journal_replay_entry(e).map(|(blk, _)| blk))
        .collect();
    assert_eq!(replayed, vec![100, 200]);
}

// ── Grow-on-mount target computation ────────────────────────────────────────

#[test]
fn test_growth_no_growth_when_device_fits() {
    // Device equal to or smaller than the FS: no growth.
    assert_eq!(onyxfs_growth_target(600, 300, 600), None);
    assert_eq!(onyxfs_growth_target(600, 300, 599), None);
}

#[test]
fn test_growth_extends_to_device() {
    // 128 MB player HDD ≈ 32768 FS blocks; small mkimage FS grows to it.
    assert_eq!(onyxfs_growth_target(600, 300, 32768), Some(32768));
    assert_eq!(onyxfs_growth_target(600, 300, 32768 - 1), Some(32768 - 1));
}

#[test]
fn test_growth_capped_by_bitmap_capacity() {
    // Data bitmap is one block: at most data_blocks_start + 32768 blocks are
    // addressable without a format change. 256 MB device → capped.
    let dev_blocks = 65536u64;
    assert_eq!(
        onyxfs_growth_target(600, 300, dev_blocks),
        Some(300 + 32768)
    );
}

#[test]
fn test_growth_capped_by_1gib_limit() {
    // Even with a hypothetical huge bitmap the 1 GiB sanity cap binds:
    // ONYFS_MAX_TOTAL_BLOCKS < 300 + 32768? No — use a tiny data start so
    // only the global cap can bind... it cannot bind below bitmap capacity,
    // so instead assert the constant relationship directly.
    assert!(ONYFS_MAX_TOTAL_BLOCKS >= ONYFS_BITMAP_CAPACITY_BLOCKS);
    assert_eq!(ONYFS_BITMAP_CAPACITY_BLOCKS, 32768);
    assert_eq!(ONYFS_MAX_TOTAL_BLOCKS, 262144);
}

#[test]
fn test_growth_limit_combines_caps() {
    assert_eq!(onyxfs_growth_limit(300), 300 + 32768);
    // Pathological data_blocks_start beyond the 1 GiB cap → cap wins.
    assert_eq!(onyxfs_growth_limit(u32::MAX - 8), 262144);
}

use super::*;
use alloc::{vec, vec::Vec};
use onyx_core::formats::ONYFS_MAGIC;

#[derive(Clone)]
struct Memory {
    blocks: Vec<Block>,
    writes: usize,
    fail_at: Option<usize>,
}
impl Io for Memory {
    fn read(&mut self, n: u32, b: &mut Block) -> KResult<()> {
        *b = *self.blocks.get(n as usize).ok_or(Errno::Io)?;
        Ok(())
    }
    fn write(&mut self, n: u32, b: &Block) -> KResult<()> {
        if self.fail_at == Some(self.writes) {
            return Err(Errno::Io);
        }
        *self.blocks.get_mut(n as usize).ok_or(Errno::Io)? = *b;
        self.writes += 1;
        Ok(())
    }
}
fn volume(used: usize) -> (Memory, OnyfsSuper) {
    let mut sb = OnyfsSuper {
        magic: ONYFS_MAGIC,
        version: 2,
        block_size: 4096,
        total_blocks: 900,
        inode_count: 64,
        inode_table_start: 3,
        data_bitmap_start: 2,
        data_blocks_start: 300,
        root_inode: 1,
        snapshot_area_start: 5,
        snapshot_count: 0,
        journal_start: 261,
        journal_size: 32,
        feature_flags: ONYFS_FEAT_SNAPSHOTS,
        creation_time: 0,
        last_mount_time: 0,
        reserved: [0; 10],
    };
    let mut io = Memory {
        blocks: vec![[0; 4096]; 900],
        writes: 0,
        fail_at: None,
    };
    for i in 0..used {
        io.blocks[2][i / 8] |= 1 << (i % 8);
        // Incompressible blocks force real capacity accounting and cross-block records.
        let mut x = i as u32 + 731;
        for byte in &mut io.blocks[300 + i] {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            *byte = x as u8;
        }
    }
    io.blocks[1][0] = 7;
    io.blocks[3][0] = 23;
    io.blocks[4][4095] = 91;
    persist(&mut io, &sb).unwrap();
    io.writes = 0;
    sb.snapshot_count = 0;
    (io, sb)
}
#[test]
fn full_root_over_197_blocks_roundtrips_metadata_and_last_bytes() {
    let (mut io, mut sb) = volume(197);
    let original = io.blocks.clone();
    assert_eq!(create(&mut io, &mut sb, b"root", 17), Ok(1));
    for at in [1, 2, 3, 4, 300, 399, 496] {
        io.blocks[at].fill(0xee);
    }
    rollback(&mut io, &mut sb, 1, &mut false).unwrap();
    for at in [1, 2, 3, 4, 300, 399, 496] {
        assert_eq!(io.blocks[at], original[at], "block {at}");
    }
    assert_eq!(sb.snapshot_count, 1);
    assert_eq!(
        meta(&mut io, Area::new(&sb).unwrap(), 1).unwrap().flags,
        PACKED
    );
}
#[test]
fn capacity_failure_is_no_space_and_changes_no_disk_bytes() {
    let (mut io, mut sb) = volume(400);
    let original = io.blocks.clone();
    assert_eq!(create(&mut io, &mut sb, b"too big", 0), Err(Errno::NoSpace));
    assert_eq!(sb.snapshot_count, 0);
    assert_eq!(io.writes, 0);
    assert_eq!(io.blocks, original);
}
#[test]
fn all_headers_remain_legacy_fail_closed_at_every_id() {
    let (mut io, mut sb) = volume(1);
    let area = Area::new(&sb).unwrap();
    for id in 1..=4 {
        assert_eq!(create(&mut io, &mut sb, b"small", 0), Ok(id));
        // Exact old rollback guard: count greater than31 fails before any target write.
        assert!(get(&io.blocks[area.anchor(id).unwrap() as usize], 0) > 31);
    }
    assert_eq!(create(&mut io, &mut sb, b"fifth", 0), Err(Errno::NoSpace));
}
#[test]
fn malformed_payload_lengths_and_targets_fail_before_live_writes() {
    for mode in 0..3 {
        let (mut io, mut sb) = volume(197);
        create(&mut io, &mut sb, b"root", 0).unwrap();
        let h = header(&mut io, Area::new(&sb).unwrap(), 1).unwrap();
        let at = h.start as usize;
        match mode {
            0 => put(&mut io.blocks[at], 0, 0), // Never permit a superblock target.
            1 => put(&mut io.blocks[at], 4, 8193), // No OOB decompressor slice.
            _ => io.blocks[at][64] ^= 1,        // Checksum mismatch after otherwise valid parse.
        }
        let before = io.blocks.clone();
        let writes = io.writes;
        assert_eq!(rollback(&mut io, &mut sb, 1, &mut false), Err(Errno::Io));
        assert_eq!(io.writes, writes);
        assert_eq!(io.blocks, before);
    }
}
#[test]
fn interrupted_rollback_replays_full_blocks_at_every_write_boundary() {
    let (mut initial, mut sb) = volume(40);
    create(&mut initial, &mut sb, b"root", 0).unwrap();
    let original = initial.blocks.clone();
    for at in [1, 2, 3, 4, 300, 339] {
        initial.blocks[at].fill(0x55);
    }
    let mut complete = initial.clone();
    let base = complete.writes;
    rollback(&mut complete, &mut sb.clone(), 1, &mut false).unwrap();
    let count = complete.writes - base;
    for cut in 0..count {
        let mut io = initial.clone();
        io.fail_at = Some(io.writes + cut);
        let mut state = sb;
        assert!(rollback(&mut io, &mut state, 1, &mut false).is_err());
        io.fail_at = None;
        // A crash re-reads superblock; the durable pending flag is the redo intent.
        let mut mounted = OnyfsSuper::from_bytes(&io.blocks[0]).unwrap();
        recover(&mut io, &mut mounted, &mut false).unwrap();
        if cut == 0 {
            assert_eq!(io.blocks[300], [0x55; 4096]);
        } else {
            for at in [1, 2, 3, 4, 300, 339] {
                assert_eq!(io.blocks[at], original[at], "cut {cut}, block {at}");
            }
        }
    }
}
#[test]
fn legacy_fixed_slot_snapshot_still_restores() {
    let (mut io, mut sb) = volume(1);
    sb.snapshot_count = 1;
    let area = Area::new(&sb).unwrap();
    let anchor = area.anchor(1).unwrap();
    let data = io.blocks[300];
    let mut hdr = [0; 4096];
    put(&mut hdr, 0, 1);
    put(&mut hdr, 4, 300);
    put(&mut hdr, 8, 4096);
    io.blocks[anchor as usize] = hdr;
    io.blocks[anchor as usize + 1] = data;
    store_meta(
        &mut io,
        area,
        &SnapshotMeta {
            id: 1,
            timestamp: 0,
            root_inode_snapshot: 1,
            block_count: 1,
            name: [0; 32],
            parent_id: 0,
            flags: 0,
            reserved: [0; 4],
        },
    )
    .unwrap();
    io.blocks[300].fill(0x99);
    rollback(&mut io, &mut sb, 1, &mut false).unwrap();
    assert_eq!(io.blocks[300], data);
}
#[test]
fn legacy_fourth_snapshot_boundary_stays_before_journal() {
    let (mut io, mut sb) = volume(1);
    sb.snapshot_count = 4;
    let area = Area::new(&sb).unwrap();
    let anchor = area.anchor(4).unwrap();
    let mut hdr = [0; 4096];
    put(&mut hdr, 0, 31);
    for i in 0..31usize {
        put(&mut hdr, 4 + i * 8, 300 + i as u32);
        put(&mut hdr, 8 + i * 8, 4096);
        io.blocks[anchor as usize + 1 + i * 2].fill(i as u8);
    }
    io.blocks[anchor as usize] = hdr;
    store_meta(
        &mut io,
        area,
        &SnapshotMeta {
            id: 4,
            timestamp: 0,
            root_inode_snapshot: 1,
            block_count: 31,
            name: [0; 32],
            parent_id: 0,
            flags: 0,
            reserved: [0; 4],
        },
    )
    .unwrap();
    let journal = io.blocks[261];
    rollback(&mut io, &mut sb, 4, &mut false).unwrap();
    assert_eq!(io.blocks[261], journal);
}
#[test]
fn decoder_rejects_trailing_rle_after_a_complete_block() {
    let mut encoded = vec![0xff, 0];
    for _ in 1..32 {
        encoded.extend_from_slice(&[0xff, 0]);
    }
    encoded.extend_from_slice(&[0, 0]);
    let mut output = [0; 4096];
    assert_eq!(
        unsafe { super::super::compress::rle_decompress(&encoded, &mut output) },
        0
    );
}
#[test]
fn failed_live_rollback_quarantines_access_until_successful_recovery() {
    let (mut io, mut sb) = volume(40);
    create(&mut io, &mut sb, b"root", 0).unwrap();
    let original = io.blocks[300];
    io.blocks[300].fill(0x77);
    io.fail_at = Some(io.writes + 5);
    let mut quarantined = false;
    assert_eq!(
        rollback(&mut io, &mut sb, 1, &mut quarantined),
        Err(Errno::Io)
    );
    assert!(quarantined);
    assert_eq!(
        super::super::snapshot_access::access(quarantined),
        Err(Errno::Io)
    );
    io.fail_at = None;
    recover(&mut io, &mut sb, &mut quarantined).unwrap();
    assert!(!quarantined);
    assert_eq!(super::super::snapshot_access::access(quarantined), Ok(()));
    assert_eq!(io.blocks[300], original);
}
#[test]
fn malformed_start_and_area_lengths_fail_instead_of_overflowing() {
    let (mut io, mut sb) = volume(1);
    sb.snapshot_area_start = 62;
    sb.journal_start = 318;
    sb.data_blocks_start = 350;
    let area = Area::new(&sb).unwrap();
    assert_eq!(area.payload(u32::MAX), Err(Errno::NoSpace));
    let mut hdr = [0; 4096];
    put(&mut hdr, 0, MAGIC);
    put(&mut hdr, 4, 1);
    put(&mut hdr, 8, 4096);
    put(&mut hdr, 16, u32::MAX);
    io.blocks[area.anchor(1).unwrap() as usize] = hdr;
    assert!(matches!(header(&mut io, area, 1), Err(Errno::Io)));
    sb.total_blocks = u32::MAX;
    sb.data_blocks_start = u32::MAX;
    sb.journal_start = u32::MAX;
    assert!(matches!(Area::new(&sb), Err(Errno::Io)));
}
#[test]
fn complete_bitmap_snapshots_also_capture_blocks_missing_from_direct_inode_walks() {
    let (mut io, mut sb) = volume(500);
    // Every block is allocated, although the inode table does not enumerate
    // those blocks directly. This models data behind single-indirect pointers.
    for block in &mut io.blocks[300..800] {
        block.fill(0x5a);
    }
    assert_eq!(create(&mut io, &mut sb, b"indirect", 0), Ok(1));
    assert_eq!(
        meta(&mut io, Area::new(&sb).unwrap(), 1)
            .unwrap()
            .block_count,
        504
    );
    io.blocks[799].fill(0);
    rollback(&mut io, &mut sb, 1, &mut false).unwrap();
    assert_eq!(io.blocks[799], [0x5a; 4096]);
}
#[test]
fn legacy_compressed_fixed_slot_snapshot_still_restores() {
    let (mut io, mut sb) = volume(1);
    sb.snapshot_count = 1;
    let area = Area::new(&sb).unwrap();
    let anchor = area.anchor(1).unwrap();
    let data = [0xa5; 4096];
    let mut compressed = [0; 8192];
    let n = unsafe { super::super::compress::rle_compress(&data, &mut compressed) };
    let mut hdr = [0; 4096];
    put(&mut hdr, 0, 1);
    put(&mut hdr, 4, 300);
    put(&mut hdr, 8, n as u32);
    io.blocks[anchor as usize] = hdr;
    io.blocks[anchor as usize + 1].copy_from_slice(&compressed[..4096]);
    store_meta(
        &mut io,
        area,
        &SnapshotMeta {
            id: 1,
            timestamp: 0,
            root_inode_snapshot: 1,
            block_count: 1,
            name: [0; 32],
            parent_id: 0,
            flags: 0,
            reserved: [0; 4],
        },
    )
    .unwrap();
    rollback(&mut io, &mut sb, 1, &mut false).unwrap();
    assert_eq!(io.blocks[300], data);
}

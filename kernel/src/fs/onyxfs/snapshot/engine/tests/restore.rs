use super::*;

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
        unsafe { crate::fs::onyxfs::snapshot::engine::compress::rle_decompress(&encoded, &mut output) },
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
        crate::fs::onyxfs::snapshot::access::access(quarantined),
        Err(Errno::Io)
    );
    io.fail_at = None;
    recover(&mut io, &mut sb, &mut quarantined).unwrap();
    assert!(!quarantined);
    assert_eq!(crate::fs::onyxfs::snapshot::access::access(quarantined), Ok(()));
    assert_eq!(io.blocks[300], original);
}

#[test]
fn legacy_compressed_fixed_slot_snapshot_still_restores() {
    let (mut io, mut sb) = volume(1);
    sb.snapshot_count = 1;
    let area = Area::new(&sb).unwrap();
    let anchor = area.anchor(1).unwrap();
    let data = [0xa5; 4096];
    let mut compressed = [0; 8192];
    let n = unsafe { crate::fs::onyxfs::snapshot::engine::compress::rle_compress(&data, &mut compressed) };
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

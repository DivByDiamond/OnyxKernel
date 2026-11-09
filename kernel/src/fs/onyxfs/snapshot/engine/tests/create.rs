use super::*;

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

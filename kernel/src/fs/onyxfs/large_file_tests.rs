//! Regression tests for files that spill into the single-indirect block
//! (more than ONYFS_DIRECT_BLKS blocks): growth, truncation, unlink and
//! reuse of freed blocks, run against an in-memory block device.
use super::tests::{SERIAL, support::*};
use super::*;
use onyx_core::formats::ONYFS_ROOT_INO;

#[test]
fn indirect_file_grow_truncate_unlink_reuse() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    // SAFETY: a single test function owns the global filesystem state, so
    // there is no concurrent onyxfs access.
    unsafe {
        mkfs();
        let base = used_data_blocks();
        let ino = create(ONYFS_ROOT_INO, b"big", 0o100644).unwrap();
        let base_with_dir = used_data_blocks();

        // Grow well past 10 direct blocks and past 45056 bytes (11 blocks).
        let data = pattern(400_000, 1);
        put(ino, &data, 4096);
        assert_eq!(size_of(ino), 400_000);
        assert_eq!(get(ino, data.len()), data);
        // 98 data blocks + 1 indirect block.
        assert_eq!(used_data_blocks(), base_with_dir + 98 + 1);

        // O_TRUNC style truncation releases everything.
        truncate(ino).unwrap();
        assert_eq!(size_of(ino), 0);
        assert_eq!(used_data_blocks(), base_with_dir);
        let mut b = [0u8; 16];
        assert_eq!(read(ino, b.as_mut_ptr(), 0, 16).unwrap(), 0);

        // Overwrite with different content: freed blocks are reused.
        let data2 = pattern(300_000, 77);
        put(ino, &data2, 4096);
        assert_eq!(get(ino, data2.len()), data2);

        // Rewrite in place with odd-sized chunks.
        let data3 = pattern(300_000, 5);
        put(ino, &data3, 1000);
        assert_eq!(get(ino, data3.len()), data3);

        // Shrink inside the indirect range keeps the prefix.
        truncate_to_length(ino, 50_000).unwrap();
        assert_eq!(size_of(ino), 50_000);
        assert_eq!(get(ino, 50_000), data3[..50_000]);
        assert_eq!(used_data_blocks(), base_with_dir + 13 + 1);

        // Shrink into the direct range drops the indirect block.
        truncate_to_length(ino, 20_000).unwrap();
        assert_eq!(get(ino, 20_000), data3[..20_000]);
        assert_eq!(used_data_blocks(), base_with_dir + 5);

        // Extend again: new area reads as zeros, nothing is aliased.
        truncate_to_length(ino, 200_000).unwrap();
        let ext = get(ino, 200_000);
        assert_eq!(ext[..20_000], data3[..20_000]);
        assert!(ext[20_000..].iter().all(|&x| x == 0));

        // Another file grown while the first one exists, then unlink both.
        let ino2 = create(ONYFS_ROOT_INO, b"other", 0o100644).unwrap();
        let o = pattern(120_000, 9);
        put(ino2, &o, 4096);
        assert_eq!(get(ino2, o.len()), o);
        assert_eq!(get(ino, 20_000), data3[..20_000]);

        unlink(b"/big").unwrap();
        unlink(b"/other").unwrap();
        assert_eq!(used_data_blocks(), base_with_dir);

        // Space returned by unlink is reusable for a fresh large file.
        let ino3 = create(ONYFS_ROOT_INO, b"again", 0o100644).unwrap();
        let d4 = pattern(500_000, 3);
        put(ino3, &d4, 4096);
        assert_eq!(get(ino3, d4.len()), d4);
        unlink(b"/again").unwrap();
        assert_eq!(used_data_blocks(), base_with_dir);
        assert!(base_with_dir >= base);
    }
}

#[test]
fn create_over_existing_file_reuses_inode_and_frees_blocks() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    // SAFETY: a single test function owns the global filesystem state.
    unsafe {
        mkfs();
        let ino = create(ONYFS_ROOT_INO, b"f", 0o100644).unwrap();
        let base = used_data_blocks();
        let data = pattern(300_000, 4);
        put(ino, &data, 512);
        assert_eq!(get(ino, data.len()), data);

        // Re-creating the name (what cp does over an existing target) must
        // start from an empty file and give every old block back.
        let again = create(ONYFS_ROOT_INO, b"f", 0o100644).unwrap();
        assert_eq!(again, ino);
        assert_eq!(size_of(again), 0);
        assert_eq!(used_data_blocks(), base);
        let small = pattern(5_000, 8);
        put(again, &small, 512);
        assert_eq!(get(again, small.len()), small);

        // Repeated rewrites of a large file do not leak space.
        for round in 0..6u8 {
            let id = create(ONYFS_ROOT_INO, b"f", 0o100644).unwrap();
            let d = pattern(400_000, round);
            put(id, &d, 512);
            assert_eq!(get(id, d.len()), d);
        }
        unlink(b"/f").unwrap();
        assert_eq!(used_data_blocks(), base);
    }
}

#[test]
fn shrink_then_extend_within_retained_block_zeroes_from_old_eof() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    // SAFETY: SERIAL excludes the other disk scenarios; run the host suite
    // with --test-threads=1 to exclude other users of filesystem globals.
    unsafe {
        for old_eof in [2_000usize, 50_000] {
            mkfs();
            let ino = create(ONYFS_ROOT_INO, b"tail", 0o100644).unwrap();
            let block_end = old_eof.div_ceil(ONYFS_BLOCK_SIZE) * ONYFS_BLOCK_SIZE;
            let data = pattern(block_end, 19);
            put(ino, &data, ONYFS_BLOCK_SIZE);
            let allocated = used_data_blocks();
            truncate_to_length(ino, old_eof as u64).unwrap();
            // Two extensions exercise successive EOFs within the same block.
            for end in [old_eof + 17, block_end] {
                truncate_to_length(ino, end as u64).unwrap();
                let out = get(ino, end);
                assert_eq!(size_of(ino), end as u64);
                assert_eq!(out[..old_eof], data[..old_eof]);
                assert!(out[old_eof..].iter().all(|&b| b == 0), "EOF {old_eof}");
                assert_eq!(used_data_blocks(), allocated);
            }
        }
    }
}

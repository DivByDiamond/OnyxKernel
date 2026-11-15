use super::super::*;
use super::{SERIAL, io, support::*};
use onyx_core::formats::ONYFS_ROOT_INO;

#[test]
fn truncate_crash_at_every_write_preserves_ownership_and_allocation() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    // SAFETY: SERIAL owns the global test volume and fault controls.
    unsafe {
        mkfs();
        let ino = create(ONYFS_ROOT_INO, b"big", 0o100644).unwrap();
        let data = pattern(300_000, 9);
        put(ino, &data, 4096);
        let pristine = io::snapshot();
        io::reset_faults();
        truncate(ino).unwrap();
        let count = io::writes();
        for cut in 0..=count {
            TEST_DISK = Some(pristine.clone());
            G_JOURNAL_HEAD = 0;
            G_SNAPSHOT_QUARANTINED = false;
            io::fail_after(cut);
            let _ = truncate(ino);
            io::reset_faults();
            G_JOURNAL_HEAD = 0; // Simulate a fresh boot with no RAM state.
            G_SNAPSHOT_QUARANTINED = false;
            journal::journal_recover().unwrap();
            let size = size_of(ino) as usize;
            assert!(size == 0 || size == data.len(), "cut {cut}: size {size}");
            // Reallocate enough blocks to overwrite every mistakenly freed one.
            let other = create(ONYFS_ROOT_INO, b"other", 0o100644).unwrap();
            put(other, &pattern(300_000, 3), 4096);
            if size != 0 {
                assert!(
                    get(ino, size) == data,
                    "reachable blocks reused at cut {cut}"
                );
            }
        }
    }
}

#[test]
fn indirect_growth_crash_at_every_write_never_leaks_allocations() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    // SAFETY: SERIAL owns the global test volume and fault controls.
    unsafe {
        mkfs();
        let ino = create(ONYFS_ROOT_INO, b"grow", 0o100644).unwrap();
        let base = used_data_blocks();
        let data = pattern(160_000, 7);
        let pristine = io::snapshot();
        io::reset_faults();
        write(ino, data.as_ptr(), 0, data.len() as u32).unwrap();
        let count = io::writes();
        for cut in 0..=count {
            TEST_DISK = Some(pristine.clone());
            G_JOURNAL_HEAD = 0;
            G_SNAPSHOT_QUARANTINED = false;
            io::fail_after(cut);
            let _ = write(ino, data.as_ptr(), 0, data.len() as u32);
            io::reset_faults();
            G_JOURNAL_HEAD = 0;
            G_SNAPSHOT_QUARANTINED = false;
            journal::journal_recover().unwrap();
            let size = size_of(ino) as usize;
            assert!(size == 0 || size == data.len(), "cut {cut}");
            let expected = if size == 0 { base } else { base + 41 };
            assert_eq!(used_data_blocks(), expected, "leak at cut {cut}");
            if size != 0 {
                assert!(get(ino, size) == data, "data at cut {cut}");
            }
        }
    }
}

#[test]
fn rename_between_directories_is_atomic_at_every_metadata_write() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    // SAFETY: SERIAL owns the global volume and each simulated reboot.
    unsafe {
        mkfs();
        let left = mkdir(ONYFS_ROOT_INO, b"left").unwrap();
        mkdir(ONYFS_ROOT_INO, b"right").unwrap();
        create(left, b"file", 0o100644).unwrap();
        let pristine = io::snapshot();
        io::reset_faults();
        rename(b"/left/file", b"/right/file").unwrap();
        let count = io::writes();
        for cut in 0..=count {
            TEST_DISK = Some(pristine.clone());
            G_JOURNAL_HEAD = 0;
            G_SNAPSHOT_QUARANTINED = false;
            io::fail_after(cut);
            let _ = rename(b"/left/file", b"/right/file");
            io::reset_faults();
            G_JOURNAL_HEAD = 0;
            G_SNAPSHOT_QUARANTINED = false;
            journal::journal_recover().unwrap();
            let mut st = OnyfsStat::default();
            let old = lookup(b"left/file", &mut st).is_ok();
            let new = lookup(b"right/file", &mut st).is_ok();
            assert_ne!(old, new, "lost or duplicate dirent at cut {cut}");
        }
    }
}

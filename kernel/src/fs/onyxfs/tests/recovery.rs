use super::super::*;
use super::{SERIAL, io, support::*};
use onyx_core::errno::Errno;
use onyx_core::formats::ONYFS_ROOT_INO;

#[test]
fn full_block_undo_checks_every_image_before_restoring_anything() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    // SAFETY: SERIAL owns all test volume state and raw test I/O.
    unsafe {
        mkfs();
        for block in [100, 101] {
            raw_write_block(block, &[3; ONYFS_BLOCK_SIZE]).unwrap();
            journal::journal_before_write(block).unwrap();
            write_block(block, &[7; ONYFS_BLOCK_SIZE]).unwrap();
        }
        // Damage the LAST byte of the second before-image. Recovery must
        // validate it before it restores even the first, valid image.
        io::flip((4 + 2) * ONYFS_BLOCK_SIZE + ONYFS_BLOCK_SIZE - 1);
        assert_eq!(journal::journal_recover(), Err(Errno::Io));
        let mut out = [0; ONYFS_BLOCK_SIZE];
        raw_read_block(100, &mut out).unwrap();
        assert!(out.iter().all(|&b| b == 7));
        io::flip((4 + 2) * ONYFS_BLOCK_SIZE + ONYFS_BLOCK_SIZE - 1);
        journal::journal_recover().unwrap();
        for block in [100, 101] {
            raw_read_block(block, &mut out).unwrap();
            assert!(out.iter().all(|&b| b == 3));
        }
    }
}

#[test]
fn interrupted_undo_can_be_retried_at_every_restore_write() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    // SAFETY: SERIAL owns the test disk, globals and injection controls.
    unsafe {
        mkfs();
        let ino = create(ONYFS_ROOT_INO, b"f", 0o100644).unwrap();
        let original = pattern(300_000, 9);
        put(ino, &original, 4096);
        io::fail_after(12);
        assert!(truncate(ino).is_err());
        let dirty = io::snapshot();
        io::reset_faults();
        G_SNAPSHOT_QUARANTINED = false;
        journal::journal_recover().unwrap();
        let count = io::writes();
        for cut in 0..=count {
            TEST_DISK = Some(dirty.clone());
            G_SNAPSHOT_QUARANTINED = false;
            io::fail_after(cut);
            let _ = journal::journal_recover();
            io::reset_faults();
            G_SNAPSHOT_QUARANTINED = false;
            journal::journal_recover().unwrap();
            assert!(get(ino, original.len()) == original, "restore cut {cut}");
        }
    }
}

#[test]
fn failed_commit_cannot_reuse_stale_images_for_a_shorter_transaction() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    // SAFETY: SERIAL owns the global volume across injected faults/reboots.
    unsafe {
        mkfs();
        for block in [100, 101, 102] {
            raw_write_block(block, &[3; ONYFS_BLOCK_SIZE]).unwrap();
            journal::journal_before_write(block).unwrap();
            write_block(block, &[7; ONYFS_BLOCK_SIZE]).unwrap();
        }
        io::fail_after(0); // Header clear fails after all home writes.
        assert!(journal::journal_commit().is_err());
        io::reset_faults();
        assert!(journal::journal_before_write(103).is_err());
        G_SNAPSHOT_QUARANTINED = false; // Reboot, then full recovery.
        journal::journal_recover().unwrap();
        journal::journal_before_write(100).unwrap();
        write_block(100, &[9; ONYFS_BLOCK_SIZE]).unwrap();
        journal::journal_commit().unwrap();
        journal::journal_before_write(101).unwrap();
        write_block(101, &[11; ONYFS_BLOCK_SIZE]).unwrap();
        journal::journal_recover().unwrap();
        let mut out = [0; ONYFS_BLOCK_SIZE];
        raw_read_block(100, &mut out).unwrap();
        assert!(out.iter().all(|&b| b == 9));
        raw_read_block(101, &mut out).unwrap();
        assert!(out.iter().all(|&b| b == 3));
    }
}

#[test]
fn full_log_rolls_back_instead_of_committing_a_partial_operation() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    // SAFETY: SERIAL excludes every other test filesystem user.
    unsafe {
        mkfs();
        let result = journal::transaction(|| {
            for block in 100..132 {
                journal::journal_before_write(block)?;
                write_block(block, &[7; ONYFS_BLOCK_SIZE])?;
            }
            Ok(())
        });
        assert_eq!(result, Err(Errno::NoSpace));
        let mut out = [0; ONYFS_BLOCK_SIZE];
        for block in 100..132 {
            read_block(block, &mut out).unwrap();
            assert!(out.iter().all(|&b| b == 0));
        }
        journal::transaction(|| create(ONYFS_ROOT_INO, b"next", 0o100644)).unwrap();
    }
}

#[test]
fn snapshot_failure_keeps_its_quarantine_outside_metadata_undo() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    // SAFETY: SERIAL owns globals; with_mount_recovery supplies FS_LOCK.
    unsafe {
        mkfs();
        let result: KResult<()> =
            crate::fs::vfs::with_mount_recovery(crate::fs::vfs::MNT_ROOT, || {
                G_SNAPSHOT_QUARANTINED = true;
                Err(Errno::Io)
            });
        assert_eq!(result, Err(Errno::Io));
        let quarantined = G_SNAPSHOT_QUARANTINED;
        assert!(quarantined);
        assert!(create(ONYFS_ROOT_INO, b"blocked", 0o100644).is_err());
        G_SNAPSHOT_QUARANTINED = false;
    }
}

#[test]
fn secondary_mount_failure_does_not_quarantine_the_root_volume() {
    use crate::fs::vfs::mount::{G_MOUNTS, MountEntry, MountFs};
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    // SAFETY: SERIAL owns globals; mount operations acquire FS_LOCK.
    unsafe {
        mkfs();
        let slot = 3;
        let original = G_MOUNTS[slot];
        let mut secondary = save_ctx();
        secondary.journal_head = 0;
        G_MOUNTS[slot] = MountEntry {
            ctx: Some(MountFs::Onyx(secondary)),
            ..MountEntry::EMPTY
        };
        let result: KResult<()> = crate::fs::vfs::with_mount_recovery(slot, || {
            G_SNAPSHOT_QUARANTINED = true;
            Err(Errno::Io)
        });
        assert_eq!(result, Err(Errno::Io));
        let root_quarantined = G_SNAPSHOT_QUARANTINED;
        assert!(!root_quarantined);
        let Some(MountFs::Onyx(saved)) = G_MOUNTS[slot].ctx else {
            panic!("lost mount");
        };
        assert!(saved.snapshot_quarantined);
        G_MOUNTS[slot] = original;
        create(ONYFS_ROOT_INO, b"root-ok", 0o100644).unwrap();
    }
}

#[test]
fn a_failed_read_only_lookup_does_not_write_a_journal_header() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    // SAFETY: SERIAL owns the test volume; the closure changes no metadata.
    unsafe {
        mkfs();
        io::reset_faults();
        let result: KResult<()> = journal::transaction(|| Err(Errno::NoEnt));
        assert_eq!(result, Err(Errno::NoEnt));
        assert_eq!(io::writes(), 0);
    }
}

#[test]
fn an_internal_create_commit_is_deferred_until_the_outer_operation_succeeds() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    // SAFETY: SERIAL owns disk and globals throughout the compound operation.
    unsafe {
        mkfs();
        let base = used_data_blocks();
        let result: KResult<()> = journal::transaction(|| {
            create(ONYFS_ROOT_INO, b"partial", 0o100644)?;
            Err(Errno::NoSpace)
        });
        assert_eq!(result, Err(Errno::NoSpace));
        let mut st = OnyfsStat::default();
        assert!(lookup(b"partial", &mut st).is_err());
        assert_eq!(used_data_blocks(), base);
        journal::transaction(|| create(ONYFS_ROOT_INO, b"complete", 0o100644)).unwrap();
    }
}

#[test]
fn an_ignored_nested_mutation_error_still_aborts_the_outer_transaction() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    // SAFETY: SERIAL owns the volume and all nested mutation state.
    unsafe {
        mkfs();
        let result = journal::transaction(|| {
            let _ = create(0, b"invalid-parent", 0o100644);
            Ok(())
        });
        assert_eq!(result, Err(Errno::Inval));
        assert_eq!(create(ONYFS_ROOT_INO, b"valid", 0o100644).unwrap(), 2);
    }
}

#[test]
fn snapshot_protocol_cannot_nest_in_an_unfinished_metadata_operation() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    // SAFETY: SERIAL owns the volume; the rejected snapshot performs no I/O.
    unsafe {
        mkfs();
        assert_eq!(
            journal::transaction(|| snapshot_create(b"nested")),
            Err(Errno::Busy)
        );
        create(ONYFS_ROOT_INO, b"still-safe", 0o100644).unwrap();
    }
}

#[test]
fn damaged_undo_magic_is_not_treated_as_an_empty_legacy_log() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    // SAFETY: SERIAL owns the disk and the deliberate header corruption.
    unsafe {
        mkfs();
        journal::journal_before_write(100).unwrap();
        write_block(100, &[7; ONYFS_BLOCK_SIZE]).unwrap();
        io::flip(4 * ONYFS_BLOCK_SIZE);
        assert_eq!(journal::journal_recover(), Err(Errno::Io));
        io::flip(4 * ONYFS_BLOCK_SIZE);
        journal::journal_recover().unwrap();
        let mut out = [0; ONYFS_BLOCK_SIZE];
        raw_read_block(100, &mut out).unwrap();
        assert!(out.iter().all(|&b| b == 0));
    }
}

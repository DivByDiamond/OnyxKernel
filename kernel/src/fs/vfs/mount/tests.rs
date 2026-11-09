use super::*;

/// Index of the first free (Fs::None) slot — pure helper so the scan is
/// unit-testable without touching the `static mut` table.
fn first_free_slot(mounts: &[MountEntry; MAX_MOUNTS]) -> Option<usize> {
    mounts.iter().position(|m| m.fs == Fs::None)
}

#[test]
fn test_first_free_slot_skips_pseudo_mounts() {
    let mut mounts = [MountEntry::EMPTY; MAX_MOUNTS];
    assert_eq!(first_free_slot(&mounts), Some(0));
    mounts[0].fs = Fs::Proc;
    mounts[1].fs = Fs::Ipc;
    mounts[2].fs = Fs::Devfs;
    assert_eq!(first_free_slot(&mounts), Some(3));
    for (i, m) in mounts.iter_mut().enumerate().take(MAX_MOUNTS) {
        m.fs = if i < 3 { Fs::Proc } else { Fs::Onyx };
    }
    assert_eq!(first_free_slot(&mounts), None);
}

#[test]
fn test_mnt_root_sentinel_casts_to_fd_byte() {
    // VfsFd.mnt stores the root sentinel truncated to u8; with_mount
    // treats any slot >= MAX_MOUNTS as root, so 0xFF must qualify.
    assert_eq!(MNT_ROOT as u8, 0xFF);
    assert!(MNT_ROOT >= MAX_MOUNTS);
}

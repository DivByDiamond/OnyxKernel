use super::*;
mod crash;
pub(super) mod io;
pub(super) mod support;
pub(super) static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn test_onyfs_stat_default() {
    let st = OnyfsStat::default();
    assert_eq!(st.ino, 0);
    assert_eq!(st.size, 0);
    assert_eq!(st.mode, 0);
    assert_eq!(st.mtime, 0);
    assert_eq!(st.atime, 0);
    assert_eq!(st.ctime, 0);
}

#[test]
fn test_inode_dirent_per_block_v1() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    // SAFETY: test-only; saves and restores G_VERSION around reads of it.
    // NOTE: cargo test may run tests in parallel; these two tests mutate the
    // same global, which is a benign-for-assertions race in practice.
    unsafe {
        let old = G_VERSION;
        G_VERSION = ONYFS_V1;
        assert_eq!(inodes_per_block(), ONYFS_BLOCK_SIZE / ONYFS_V1_INODE_SIZE);
        assert_eq!(inodes_per_block(), 64);
        assert_eq!(dirents_per_block(), ONYFS_BLOCK_SIZE / ONYFS_V1_DIRENT_SIZE);
        assert_eq!(dirents_per_block(), 113);
        G_VERSION = old;
    }
}

#[test]
fn test_inode_dirent_per_block_v2() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    // SAFETY: test-only; saves and restores G_VERSION around reads of it.
    // NOTE: races with test_inode_dirent_per_block_v1 if run in parallel.
    unsafe {
        let old = G_VERSION;
        G_VERSION = ONYFS_V2;
        assert_eq!(
            inodes_per_block(),
            ONYFS_BLOCK_SIZE / onyx_core::formats::OnyfsInode::SIZE
        );
        assert_eq!(inodes_per_block(), 32);
        assert_eq!(
            dirents_per_block(),
            ONYFS_BLOCK_SIZE / onyx_core::formats::OnyfsDirent::SIZE
        );
        assert_eq!(dirents_per_block(), 102);
        G_VERSION = old;
    }
}

#[test]
fn test_onyxfs_constants() {
    assert_eq!(ONYFS_V1, 1);
    assert_eq!(ONYFS_V2, 2);
    assert_eq!(ONYFS_BLOCK_SIZE, 4096);
    assert_eq!(ONYFS_V1_INODE_SIZE, 64);
    assert_eq!(ONYFS_V1_DIRENT_SIZE, 36);
    assert_eq!(onyx_core::formats::OnyfsInode::SIZE, 128);
    assert_eq!(onyx_core::formats::OnyfsDirent::SIZE, 40);
}

mod recovery;

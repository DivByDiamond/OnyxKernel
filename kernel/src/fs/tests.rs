use super::vfs::{Fs, MAX_MOUNTS, VFS_MAX_FDS};

#[test]
fn test_fs_enum_from_u32() {
    assert!(Fs::None as u32 == 0);
    assert!(Fs::Onyx as u32 == 1);
    assert!(Fs::Fat32 as u32 == 2);
    assert!(Fs::Proc as u32 == 3);
    assert!(Fs::Ipc as u32 == 4);
    assert!(Fs::Devfs as u32 == 5);
}

#[test]
fn test_vfs_limits() {
    assert_eq!(VFS_MAX_FDS, 16);
    assert_eq!(MAX_MOUNTS, 6);
}

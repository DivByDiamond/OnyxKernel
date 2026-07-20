use super::*;

#[test]
fn test_fd_token_roundtrip() {
    let idx = 5;
    let epoch = 42;
    let token = fd_token(idx, epoch);
    assert_eq!(fd_token_idx(token), idx);
    assert_eq!(fd_token_epoch(token), epoch);
}

#[test]
fn test_fd_token_boundaries() {
    let token = fd_token(0, 0);
    assert_eq!(fd_token_idx(token), 0);
    assert_eq!(fd_token_epoch(token), 0);

    let token = fd_token(0xFFFF, 0xDEAD_BEEF);
    assert_eq!(fd_token_idx(token), 0xFFFF);
    assert_eq!(fd_token_epoch(token), 0xDEAD_BEEF);
}

#[test]
fn test_fs_enum_values() {
    assert_eq!(Fs::None as u32, 0);
    assert_eq!(Fs::Onyx as u32, 1);
    assert_eq!(Fs::Fat32 as u32, 2);
    assert_eq!(Fs::Proc as u32, 3);
    assert_eq!(Fs::Ipc as u32, 4);
    assert_eq!(Fs::Devfs as u32, 5);
}

#[test]
fn test_fs_equality() {
    assert!(Fs::None == Fs::None);
    assert!(Fs::Onyx == Fs::Onyx);
    assert!(Fs::Onyx != Fs::Fat32);
    assert!(Fs::Proc != Fs::Ipc);
}

#[test]
fn test_perm_constants() {
    assert_eq!(PERM_READ, 1);
    assert_eq!(PERM_WRITE, 2);
    assert_eq!(PERM_SEEK, 4);
    assert_eq!(PERM_EXEC, 8);
    assert_eq!(PERM_ALL, 15);
    assert_eq!(PERM_ALL, PERM_READ | PERM_WRITE | PERM_SEEK | PERM_EXEC);
}

#[test]
fn test_vfs_fd_default() {
    let fd = VfsFd::default();
    assert_eq!(fd.ino, 0);
    assert_eq!(fd.size, 0);
    assert_eq!(fd.pos, 0);
    assert!(fd.fs == Fs::None);
    assert!(!fd.used);
    assert_eq!(fd.perms, 0);
    assert_eq!(fd.epoch, 0);
    assert!(!fd.cloexec);
}

#[test]
fn test_fd_token_none_components() {
    assert_eq!(fd_token_idx(FD_TOKEN_NONE) as u32, 0xFFFF_FFFF);
    assert_eq!(fd_token_epoch(FD_TOKEN_NONE), 0xFFFF_FFFF);
}

#[test]
fn test_fd_token_composed() {
    for idx in [0usize, 1, 15, 255, 0xFFFF] {
        for epoch in [0u32, 1, 42, u32::MAX] {
            let token = fd_token(idx, epoch);
            assert_eq!(fd_token_idx(token), idx);
            assert_eq!(fd_token_epoch(token), epoch);
        }
    }
}

#[test]
fn test_vfs_max_fds() {
    assert_eq!(VFS_MAX_FDS, 16);
}

#[test]
fn test_max_mounts() {
    assert_eq!(MAX_MOUNTS, 6);
}

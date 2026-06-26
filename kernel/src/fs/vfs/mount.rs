use crate::fs::{fat32, onyxfs};
use onyx_core::errno::{Errno, KResult};

use super::vnode::{Fs, MAX_MOUNTS};

#[derive(Clone, Copy)]
pub struct MountEntry {
    pub path: &'static [u8],
    pub fs: Fs,
}

pub(crate) static mut G_MOUNTS: [MountEntry; MAX_MOUNTS] = [
    MountEntry { path: b"", fs: Fs::None },
    MountEntry { path: b"", fs: Fs::None },
    MountEntry { path: b"", fs: Fs::None },
    MountEntry { path: b"", fs: Fs::None },
    MountEntry { path: b"", fs: Fs::None },
];

pub unsafe fn mount_procfs() {
    G_MOUNTS[0] = MountEntry {
        path: b"proc",
        fs: Fs::Proc,
    };
}

pub unsafe fn mount_ipcfs() {
    G_MOUNTS[1] = MountEntry {
        path: b"ipc",
        fs: Fs::Ipc,
    };
}

pub(crate) unsafe fn resolve_mount(path: &[u8]) -> (Fs, &[u8]) {
    for m in G_MOUNTS.iter() {
        if m.fs == Fs::None {
            continue;
        }
        if path == m.path {
            return (m.fs, b"");
        }
        if path.starts_with(m.path) && path.len() > m.path.len() && path[m.path.len()] == b'/' {
            let sub = &path[m.path.len() + 1..];
            return (m.fs, sub);
        }
    }
    (root_fs(), path)
}

pub(crate) static mut G_ROOT_FS: Fs = Fs::None;

pub unsafe fn mount_root(dev: usize, onyxfs_lba: u32) -> KResult<()> {
    if onyxfs::mount(dev, onyxfs_lba).is_ok() {
        *(&raw mut G_ROOT_FS) = Fs::Onyx;
        return Ok(());
    }
    if fat32::mount(dev).is_ok() {
        *(&raw mut G_ROOT_FS) = Fs::Fat32;
        return Ok(());
    }
    Err(Errno::Io)
}

pub fn root_fs() -> Fs {
    unsafe { *(&raw const G_ROOT_FS) }
}

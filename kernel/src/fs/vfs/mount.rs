use crate::fs::{fat32, onyxfs};
use onyx_core::errno::{Errno, KResult};

use super::vnode::{Fs, MAX_MOUNTS};

#[derive(Clone, Copy)]
pub struct MountEntry {
    pub path: &'static [u8],
    pub fs: Fs,
}

pub(crate) static mut G_MOUNTS: [MountEntry; MAX_MOUNTS] = [
    MountEntry {
        path: b"",
        fs: Fs::None,
    },
    MountEntry {
        path: b"",
        fs: Fs::None,
    },
    MountEntry {
        path: b"",
        fs: Fs::None,
    },
    MountEntry {
        path: b"",
        fs: Fs::None,
    },
    MountEntry {
        path: b"",
        fs: Fs::None,
    },
    MountEntry {
        path: b"",
        fs: Fs::None,
    },
];

/// # Safety
///
/// Caller contract: run once during boot-time VFS setup (srv::vfs::setup) on
/// the boot hart, before user processes (the only G_MOUNTS readers via
/// `resolve_mount`) are scheduled. No locking guards G_MOUNTS.
pub unsafe fn mount_procfs() {
    // SAFETY: one-shot boot-time write to a slot no reader can observe yet:
    // user processes only start after srv::vfs::setup completes.
    unsafe {
        G_MOUNTS[0] = MountEntry {
            path: b"proc",
            fs: Fs::Proc,
        };
    }
}

/// # Safety
///
/// Caller contract: run once during boot-time VFS setup on the boot hart,
/// before user processes are scheduled (no lock guards G_MOUNTS).
pub unsafe fn mount_ipcfs() {
    // SAFETY: one-shot boot-time write; no concurrent readers exist yet.
    unsafe {
        G_MOUNTS[1] = MountEntry {
            path: b"ipc",
            fs: Fs::Ipc,
        };
    }
}

/// # Safety
///
/// Caller contract: run once during boot-time VFS setup on the boot hart,
/// before user processes are scheduled (no lock guards G_MOUNTS).
pub unsafe fn mount_devfs() {
    // SAFETY: one-shot boot-time write; no concurrent readers exist yet.
    unsafe {
        G_MOUNTS[2] = MountEntry {
            path: b"dev",
            fs: Fs::Devfs,
        };
    }
}

/// # Safety
///
/// Caller contract: G_MOUNTS is populated once during boot-time setup and
/// only read afterwards; call only after boot-time mounting has completed
/// (or from the boot-time setup itself). There is no lock protecting the
/// table against concurrent mount-table writes.
pub(crate) unsafe fn resolve_mount(path: &[u8]) -> (Fs, &[u8]) {
    // SAFETY: read-only traversal of G_MOUNTS, whose entries are immutable
    // after boot-time mounting (see mount_* callers in srv::vfs::setup).
    unsafe {
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
}

pub(crate) static mut G_ROOT_FS: Fs = Fs::None;

/// # Safety
///
/// Caller contract: run once during boot-time VFS setup (srv::vfs::setup) on
/// the boot hart, before user processes are scheduled; G_ROOT_FS has no lock.
pub unsafe fn mount_root(dev: usize, onyxfs_lba: u32) -> KResult<()> {
    // SAFETY: boot-time one-shot initialization of G_ROOT_FS, before any
    // user process (and thus any root_fs() reader) can be scheduled.
    unsafe {
        if onyxfs::mount(dev, onyxfs_lba).is_ok() {
            G_ROOT_FS = Fs::Onyx;
            return Ok(());
        }
        if fat32::mount(dev).is_ok() {
            G_ROOT_FS = Fs::Fat32;
            return Ok(());
        }
        Err(Errno::Io)
    }
}

pub fn root_fs() -> Fs {
    // SAFETY: read of a `static mut` that is written only once during
    // boot-time mounting; word-sized enum load, no torn read possible.
    unsafe { G_ROOT_FS }
}

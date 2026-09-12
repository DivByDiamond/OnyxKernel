//! Mount table, path resolution, and root mounting.
//!
//! The secondary-mount execution machinery (FS singleton switching under
//! `FS_LOCK`) lives in the sibling `exec` module.
mod exec;

pub(crate) use exec::{is_dir_target, rel_to_abs, resolve_path, stat_target, with_mount};

use crate::fs::fat32::FatCtx;
use crate::fs::onyxfs::OnyxCtx;
use crate::fs::{fat32, onyxfs};
use onyx_core::errno::{Errno, KResult};

use super::vnode::{Fs, MAX_MOUNTS};

/// Slot sentinel for the root filesystem: root is not a `G_MOUNTS` entry,
/// and its context lives directly in the onyxfs/fat32 module singletons.
/// Stored in `VfsFd.mnt` truncated to `u8` (0xFF); `with_mount` treats any
/// slot `>= MAX_MOUNTS` as root.
pub const MNT_ROOT: usize = usize::MAX;

/// Cached filesystem context for a secondary mount. `with_mount` activates
/// the matching singleton state for the duration of one filesystem
/// operation and writes back onyxfs drift (grown superblock, journal head).
#[derive(Clone, Copy)]
pub enum MountFs {
    Onyx(OnyxCtx),
    Fat32(FatCtx),
}

#[derive(Clone, Copy)]
pub struct MountEntry {
    pub path: &'static [u8],
    pub fs: Fs,
    pub dev: usize,
    pub lba: u32,
    /// Present only for block-FS mounts created by `mount_secondary`;
    /// pseudo-filesystems (proc/ipc/dev) and free slots carry `None`.
    pub ctx: Option<MountFs>,
}

impl MountEntry {
    pub(crate) const EMPTY: MountEntry = MountEntry {
        path: b"",
        fs: Fs::None,
        dev: 0,
        lba: 0,
        ctx: None,
    };
}

pub(crate) static mut G_MOUNTS: [MountEntry; MAX_MOUNTS] = [MountEntry::EMPTY; MAX_MOUNTS];

/// Index of the first free (Fs::None) slot — pure helper so the scan is
/// unit-testable without touching the `static mut` table.
#[cfg(test)]
fn first_free_slot(mounts: &[MountEntry; MAX_MOUNTS]) -> Option<usize> {
    mounts.iter().position(|m| m.fs == Fs::None)
}

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
            dev: 0,
            lba: 0,
            ctx: None,
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
            dev: 0,
            lba: 0,
            ctx: None,
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
            dev: 0,
            lba: 0,
            ctx: None,
        };
    }
}

/// Insert a secondary block-FS mount at the first free table slot. Called
/// from boot auto-mount (srv::vfs::auto) after the filesystem context has
/// been mounted; `path` must be a leaked `mnt/blkN` slice (mount-relative,
/// no leading '/').
///
/// # Safety
///
/// Caller contract: run during boot-time VFS setup on the boot hart, before
/// user processes are scheduled. Takes `FS_LOCK` so the slot scan and write
/// are atomic with respect to `with_mount` context write-backs.
pub(crate) unsafe fn mount_secondary(
    path: &'static [u8],
    dev: usize,
    lba: u32,
    ctx: MountFs,
) -> KResult<usize> {
    // SAFETY: FS_LOCK is only taken in SIE=0 kernel context (see crate::sync
    // and vfs::with_mount); the raw pointer write targets the table only
    // while the lock is held and the shared borrow used for the scan is dead
    // before the write starts.
    unsafe {
        let fs = match ctx {
            MountFs::Onyx(_) => Fs::Onyx,
            MountFs::Fat32(_) => Fs::Fat32,
        };
        crate::fs::FS_LOCK.lock();
        let slot = {
            let mounts = core::ptr::addr_of!(G_MOUNTS).cast::<MountEntry>();
            (0..MAX_MOUNTS).find(|&i| mounts.add(i).read().fs == Fs::None)
        };
        if let Some(s) = slot {
            let pm = &raw mut G_MOUNTS;
            (*pm)[s] = MountEntry {
                path,
                fs,
                dev,
                lba,
                ctx: Some(ctx),
            };
        }
        crate::fs::FS_LOCK.unlock();
        slot.ok_or(Errno::NoMem)
    }
}

/// # Safety
///
/// Caller contract: G_MOUNTS is populated during boot-time setup (secondary
/// inserts take `FS_LOCK`; the initial proc/ipc/dev writes precede any
/// reader). Call only from kernel/syscall context. Raw reads avoid creating a
/// shared reference to the mutable static while `with_mount` may update
/// cached contexts under `FS_LOCK`; paths remain immutable after boot.
pub(crate) unsafe fn resolve_mount(path: &[u8]) -> (Fs, &[u8], usize) {
    // SAFETY: each entry is copied out by value. This avoids coexisting with
    // the raw mutable context writes performed by `with_mount` while still
    // reading the boot-stable path/fs fields.
    unsafe {
        let mounts = core::ptr::addr_of!(G_MOUNTS).cast::<MountEntry>();
        for slot in 0..MAX_MOUNTS {
            let m = mounts.add(slot).read();
            if m.fs == Fs::None {
                continue;
            }
            if path == m.path {
                return (m.fs, b"", slot);
            }
            if path.starts_with(m.path) && path.len() > m.path.len() && path[m.path.len()] == b'/' {
                let sub = &path[m.path.len() + 1..];
                return (m.fs, sub, slot);
            }
        }
        (root_fs(), path, MNT_ROOT)
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

#[cfg(test)]
mod tests {
    use super::*;

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
}

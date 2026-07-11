//! Stateful readdir — single active directory cursor (MVP).
use crate::fs::{devfs, ipcfs, onyxfs, procfs};
use onyx_core::errno::{Errno, KResult};

use super::Fs;
use super::resolve_mount;

/// readdir: stateful per-process directory listing.
/// Uses a static cursor (MVP: single active readdir at a time).
pub(super) static mut G_DIR_CURSOR_INO: u32 = 0;
pub(super) static mut G_DIR_CURSOR_IDX: u32 = 0;
pub(super) static mut G_DIR_ACTIVE: bool = false;
pub(super) static mut G_DIR_FS: Fs = Fs::None;

pub unsafe fn readdir(dir_path: &[u8], name_out: *mut u8, name_len: usize) -> KResult<bool> {
    if dir_path.is_empty() || dir_path[0] != b'/' {
        return Err(Errno::Inval);
    }
    let name = &dir_path[1..];
    let (fs, subpath) = resolve_mount(name);

    match fs {
        Fs::Proc => {
            let ino = if subpath.is_empty() || subpath == b"." {
                procfs::PROCFS_ROOT_INO
            } else {
                procfs::lookup(subpath)?
            };
            if !G_DIR_ACTIVE || G_DIR_CURSOR_INO != ino || G_DIR_FS != Fs::Proc {
                G_DIR_CURSOR_INO = ino;
                G_DIR_CURSOR_IDX = 0;
                G_DIR_ACTIVE = true;
                G_DIR_FS = Fs::Proc;
            }
            match procfs::readdir_entry(G_DIR_CURSOR_IDX, name_out, name_len) {
                Some(_ino) => {
                    G_DIR_CURSOR_IDX += 1;
                    Ok(true)
                }
                None => {
                    G_DIR_ACTIVE = false;
                    Ok(false)
                }
            }
        }
        Fs::Ipc => {
            let ino = if subpath.is_empty() || subpath == b"." {
                ipcfs::IPCFS_ROOT_INO
            } else {
                ipcfs::lookup(subpath)?
            };
            if !G_DIR_ACTIVE || G_DIR_CURSOR_INO != ino || G_DIR_FS != Fs::Ipc {
                G_DIR_CURSOR_INO = ino;
                G_DIR_CURSOR_IDX = 0;
                G_DIR_ACTIVE = true;
                G_DIR_FS = Fs::Ipc;
            }
            match ipcfs::readdir_entry(G_DIR_CURSOR_IDX, name_out, name_len) {
                Some(_ino) => {
                    G_DIR_CURSOR_IDX += 1;
                    Ok(true)
                }
                None => {
                    G_DIR_ACTIVE = false;
                    Ok(false)
                }
            }
        }
        Fs::Devfs => {
            let ino = if subpath.is_empty() || subpath == b"." {
                devfs::DEVFS_ROOT_INO
            } else {
                devfs::lookup(subpath)?
            };
            if !G_DIR_ACTIVE || G_DIR_CURSOR_INO != ino || G_DIR_FS != Fs::Devfs {
                G_DIR_CURSOR_INO = ino;
                G_DIR_CURSOR_IDX = 0;
                G_DIR_ACTIVE = true;
                G_DIR_FS = Fs::Devfs;
            }
            match devfs::readdir_entry(G_DIR_CURSOR_IDX, name_out, name_len) {
                Some(_ino) => {
                    G_DIR_CURSOR_IDX += 1;
                    Ok(true)
                }
                None => {
                    G_DIR_ACTIVE = false;
                    Ok(false)
                }
            }
        }
        _ => {
            let ino = onyxfs::resolve_dir(dir_path)?;
            if !G_DIR_ACTIVE || G_DIR_CURSOR_INO != ino || G_DIR_FS != Fs::Onyx {
                G_DIR_CURSOR_INO = ino;
                G_DIR_CURSOR_IDX = 0;
                G_DIR_ACTIVE = true;
                G_DIR_FS = Fs::Onyx;
            }
            match onyxfs::readdir_entry(G_DIR_CURSOR_INO, G_DIR_CURSOR_IDX, name_out, name_len)? {
                Some(_ino) => {
                    G_DIR_CURSOR_IDX += 1;
                    Ok(true)
                }
                None => {
                    G_DIR_ACTIVE = false;
                    Ok(false)
                }
            }
        }
    }
}

/// Read a single directory entry by inode and cursor index.
/// Used by getdents64 for fd-based directory iteration.
pub unsafe fn readdir_entry_by_ino(
    fs: Fs,
    ino: u32,
    idx: u32,
    name_out: *mut u8,
    name_len: usize,
) -> KResult<Option<u32>> {
    match fs {
        Fs::Onyx => onyxfs::readdir_entry(ino, idx, name_out, name_len),
        Fs::Proc => match procfs::readdir_entry(idx, name_out, name_len) {
            Some(d_ino) => Ok(Some(d_ino)),
            None => Ok(None),
        },
        Fs::Ipc => match ipcfs::readdir_entry(idx, name_out, name_len) {
            Some(d_ino) => Ok(Some(d_ino)),
            None => Ok(None),
        },
        Fs::Devfs => match devfs::readdir_entry(idx, name_out, name_len) {
            Some(d_ino) => Ok(Some(d_ino)),
            None => Ok(None),
        },
        _ => Err(Errno::NoSys),
    }
}

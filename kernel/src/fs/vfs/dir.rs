//! Stateful readdir — per-process directory cursor.
use crate::fs::{devfs, fat32, ipcfs, onyxfs, procfs};
use onyx_core::errno::{Errno, KResult};

use super::Fs;
use super::resolve_mount;

pub unsafe fn readdir(dir_path: &[u8], name_out: *mut u8, name_len: usize) -> KResult<bool> {
    if dir_path.is_empty() || dir_path[0] != b'/' {
        return Err(Errno::Inval);
    }
    let name = &dir_path[1..];
    let (fs, subpath) = resolve_mount(name);
    let p = crate::proc::current();

    match fs {
        Fs::Proc => {
            let ino = if subpath.is_empty() || subpath == b"." {
                procfs::PROCFS_ROOT_INO
            } else {
                procfs::lookup(subpath)?
            };
            if !p.readdir_active || p.readdir_ino != ino || p.readdir_fs != Fs::Proc {
                p.readdir_ino = ino;
                p.readdir_idx = 0;
                p.readdir_active = true;
                p.readdir_fs = Fs::Proc;
            }
            match procfs::readdir_entry(p.readdir_idx, name_out, name_len) {
                Some(_ino) => {
                    p.readdir_idx += 1;
                    Ok(true)
                }
                None => {
                    p.readdir_active = false;
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
            if !p.readdir_active || p.readdir_ino != ino || p.readdir_fs != Fs::Ipc {
                p.readdir_ino = ino;
                p.readdir_idx = 0;
                p.readdir_active = true;
                p.readdir_fs = Fs::Ipc;
            }
            match ipcfs::readdir_entry(p.readdir_idx, name_out, name_len) {
                Some(_ino) => {
                    p.readdir_idx += 1;
                    Ok(true)
                }
                None => {
                    p.readdir_active = false;
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
            if !p.readdir_active || p.readdir_ino != ino || p.readdir_fs != Fs::Devfs {
                p.readdir_ino = ino;
                p.readdir_idx = 0;
                p.readdir_active = true;
                p.readdir_fs = Fs::Devfs;
            }
            match devfs::readdir_entry(p.readdir_idx, name_out, name_len) {
                Some(_ino) => {
                    p.readdir_idx += 1;
                    Ok(true)
                }
                None => {
                    p.readdir_active = false;
                    Ok(false)
                }
            }
        }
        Fs::Fat32 => {
            let mut cluster = 0u32;
            let mut size = 0u32;
            fat32::lookup(subpath, &mut cluster, &mut size)?;
            if !p.readdir_active || p.readdir_ino != cluster || p.readdir_fs != Fs::Fat32 {
                p.readdir_ino = cluster;
                p.readdir_idx = 0;
                p.readdir_active = true;
                p.readdir_fs = Fs::Fat32;
            }
            match fat32::readdir_entry(p.readdir_ino, p.readdir_idx, name_out, name_len) {
                Some(_ino) => {
                    p.readdir_idx += 1;
                    Ok(true)
                }
                None => {
                    p.readdir_active = false;
                    Ok(false)
                }
            }
        }
        _ => {
            let ino = onyxfs::resolve_dir(dir_path)?;
            if !p.readdir_active || p.readdir_ino != ino || p.readdir_fs != Fs::Onyx {
                p.readdir_ino = ino;
                p.readdir_idx = 0;
                p.readdir_active = true;
                p.readdir_fs = Fs::Onyx;
            }
            match onyxfs::readdir_entry(p.readdir_ino, p.readdir_idx, name_out, name_len)? {
                Some(_ino) => {
                    p.readdir_idx += 1;
                    Ok(true)
                }
                None => {
                    p.readdir_active = false;
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
        Fs::Fat32 => match fat32::readdir_entry(ino, idx, name_out, name_len) {
            Some(d_ino) => Ok(Some(d_ino)),
            None => Ok(None),
        },
        _ => Err(Errno::NoSys),
    }
}

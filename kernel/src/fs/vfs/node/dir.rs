//! Stateful readdir — per-process directory cursor.
use crate::fs::{devfs, fat32, ipcfs, onyxfs, procfs};
use onyx_core::errno::{Errno, KResult};

use crate::fs::vfs::Fs;
use crate::fs::vfs::resolve_mount;

/// # Safety
///
/// Caller contract: dir_path comes from the syscall layer's parse_user_path
/// (kernel-side slice); for user callers name_out is a validated user range
/// of name_len bytes (user_ptr_ok/check_user_range upstream, translated);
/// kernel callers pass a valid kernel buffer. Mutates the readdir cursor of
/// the process current on this hart.
pub unsafe fn readdir(dir_path: &[u8], name_out: *mut u8, name_len: usize) -> KResult<bool> {
    // SAFETY: proc::current() returns the Proc current on this hart; only
    // this process's own syscall context touches its cursor fields. name_out
    // is forwarded unchanged to per-fs readdir_entry, which bound their
    // writes to name_len (see procfs/dir.rs and ipcfs::copy_name).
    unsafe {
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
}

/// Read a single directory entry by inode and cursor index.
/// Used by getdents64 for fd-based directory iteration.
///
/// # Safety
///
/// Caller contract: same buffer contract as readdir (validated/translated
/// name_out of name_len bytes for user callers); fs/ino/idx must come from a
/// live fd or a validated stat call.
pub unsafe fn readdir_entry_by_ino(
    fs: Fs,
    ino: u32,
    idx: u32,
    name_out: *mut u8,
    name_len: usize,
) -> KResult<Option<u32>> {
    // SAFETY: only forwards name_out to per-fs readdir_entry helpers that
    // bound their writes to name_len; ino/idx validity is checked by each
    // backend (bounds-checked table walks).
    unsafe {
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
}

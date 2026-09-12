//! Stateful readdir — per-process directory cursor.
use crate::fs::vfs::mount::resolve_mount;
use crate::fs::vfs::{Fs, with_mount};
use crate::fs::{devfs, fat32, ipcfs, onyxfs, procfs};
use onyx_core::errno::{Errno, KResult};

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
    // The cursor additionally records the mount slot (readdir_mnt) so two
    // mounts with colliding on-disk inos cannot share a cursor.
    unsafe {
        if dir_path.is_empty() || dir_path[0] != b'/' {
            return Err(Errno::Inval);
        }
        let name = &dir_path[1..];
        let (fs, subpath, slot) = resolve_mount(name);
        let p = crate::proc::current();

        match fs {
            Fs::Proc => {
                let ino = if subpath.is_empty() || subpath == b"." {
                    procfs::PROCFS_ROOT_INO
                } else {
                    procfs::lookup(subpath)?
                };
                if !p.readdir_active
                    || p.readdir_ino != ino
                    || p.readdir_fs != Fs::Proc
                    || p.readdir_mnt != slot as u8
                {
                    p.readdir_ino = ino;
                    p.readdir_idx = 0;
                    p.readdir_active = true;
                    p.readdir_fs = Fs::Proc;
                    p.readdir_mnt = slot as u8;
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
                if !p.readdir_active
                    || p.readdir_ino != ino
                    || p.readdir_fs != Fs::Ipc
                    || p.readdir_mnt != slot as u8
                {
                    p.readdir_ino = ino;
                    p.readdir_idx = 0;
                    p.readdir_active = true;
                    p.readdir_fs = Fs::Ipc;
                    p.readdir_mnt = slot as u8;
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
                if !p.readdir_active
                    || p.readdir_ino != ino
                    || p.readdir_fs != Fs::Devfs
                    || p.readdir_mnt != slot as u8
                {
                    p.readdir_ino = ino;
                    p.readdir_idx = 0;
                    p.readdir_active = true;
                    p.readdir_fs = Fs::Devfs;
                    p.readdir_mnt = slot as u8;
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
                let cluster = with_mount(slot, || -> KResult<u32> {
                    let mut c = 0u32;
                    let mut size = 0u32;
                    fat32::lookup(subpath, &mut c, &mut size)?;
                    Ok(c)
                })?;
                if !p.readdir_active
                    || p.readdir_ino != cluster
                    || p.readdir_fs != Fs::Fat32
                    || p.readdir_mnt != slot as u8
                {
                    p.readdir_ino = cluster;
                    p.readdir_idx = 0;
                    p.readdir_active = true;
                    p.readdir_fs = Fs::Fat32;
                    p.readdir_mnt = slot as u8;
                }
                let (cur_ino, cur_idx) = (p.readdir_ino, p.readdir_idx);
                let entry = with_mount(slot, || {
                    fat32::readdir_entry(cur_ino, cur_idx, name_out, name_len)
                });
                match entry {
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
            Fs::Onyx => {
                // resolve_dir accepts mount-relative paths (it skips leading
                // separators), so subpath works unchanged for root and
                // secondary mounts alike.
                let ino = with_mount(slot, || onyxfs::resolve_dir(subpath))?;
                if !p.readdir_active
                    || p.readdir_ino != ino
                    || p.readdir_fs != Fs::Onyx
                    || p.readdir_mnt != slot as u8
                {
                    p.readdir_ino = ino;
                    p.readdir_idx = 0;
                    p.readdir_active = true;
                    p.readdir_fs = Fs::Onyx;
                    p.readdir_mnt = slot as u8;
                }
                let (cur_ino, cur_idx) = (p.readdir_ino, p.readdir_idx);
                let entry = with_mount(slot, || {
                    onyxfs::readdir_entry(cur_ino, cur_idx, name_out, name_len)
                })?;
                match entry {
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
            Fs::None => Err(Errno::Inval),
        }
    }
}

/// Read a single directory entry by inode and cursor index.
/// Used by getdents64 for fd-based directory iteration. `mnt` is the fd's
/// mount-slot byte (`VfsFd::mnt`); block-fs entries are read through
/// `with_mount` so the correct volume context is active.
///
/// # Safety
///
/// Caller contract: same buffer contract as readdir (validated/translated
/// name_out of name_len bytes for user callers); fs/ino/idx/mnt must come
/// from a live fd or a validated stat call.
pub unsafe fn readdir_entry_by_ino(
    fs: Fs,
    ino: u32,
    idx: u32,
    name_out: *mut u8,
    name_len: usize,
    mnt: u8,
) -> KResult<Option<u32>> {
    // SAFETY: only forwards name_out to per-fs readdir_entry helpers that
    // bound their writes to name_len; ino/idx validity is checked by each
    // backend (bounds-checked table walks); with_mount fences the block-fs
    // singleton swap (FS_LOCK, SIE=0 kernel context).
    unsafe {
        let mnt = mnt as usize;
        match fs {
            Fs::Onyx => with_mount(mnt, || onyxfs::readdir_entry(ino, idx, name_out, name_len)),
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
            Fs::Fat32 => with_mount(mnt, || {
                match fat32::readdir_entry(ino, idx, name_out, name_len) {
                    Some(d_ino) => Ok(Some(d_ino)),
                    None => Ok(None),
                }
            }),
            _ => Err(Errno::NoSys),
        }
    }
}

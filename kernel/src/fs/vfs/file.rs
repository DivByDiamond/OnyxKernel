use super::{
    FdToken, Fs, G_ROOT_FS, PERM_READ, PERM_SEEK, PERM_WRITE, VFS_MAX_FDS, alloc_fd, fd_check,
    fd_clear, fd_get, fd_set, fd_token, fd_update_pos,
};
use crate::fs::{devfs, fat32, ipcfs, onyxfs, procfs};
use onyx_core::errno::{Errno, KResult};

pub unsafe fn open(path: &[u8], perms: u32) -> KResult<FdToken> {
    if path.is_empty() || path[0] != b'/' {
        return Err(Errno::Inval);
    }
    let name = &path[1..];
    let idx = alloc_fd(perms)?;

    // Check mount table first.
    let (fs, subpath) = super::resolve_mount(name);
    // Bug (fs SERIOUS #3): if the lookup below fails, we must release the
    // FD slot we just allocated — otherwise every failed open() permanently
    // leaks an FD slot, and a process that retries open() in a loop will
    // exhaust its FD table (VFS_MAX_FDS = 16). We use a closure-style
    // pattern: do the lookup, and on Err, call fd_clear(idx) before
    // propagating the error.
    let (ino, size) = match fs {
        Fs::Proc => {
            let ino = match procfs::lookup(subpath) {
                Ok(i) => i,
                Err(e) => {
                    let _ = fd_clear(idx);
                    return Err(e);
                }
            };
            let st = procfs::stat(ino)?;
            (ino, st.size)
        }
        Fs::Ipc => {
            let ino = match ipcfs::lookup(subpath) {
                Ok(i) => i,
                Err(e) => {
                    let _ = fd_clear(idx);
                    return Err(e);
                }
            };
            let st = ipcfs::stat(ino)?;
            (ino, st.size)
        }
        Fs::Devfs => {
            let ino = match devfs::lookup(subpath) {
                Ok(i) => i,
                Err(e) => {
                    let _ = fd_clear(idx);
                    return Err(e);
                }
            };
            let st = devfs::stat(ino)?;
            (ino, st.size)
        }
        _ => {
            let mut st = onyxfs::OnyfsStat::default();
            match *(&raw const G_ROOT_FS) {
                Fs::Onyx => {
                    if let Err(e) = onyxfs::lookup(name, &mut st) {
                        let _ = fd_clear(idx);
                        return Err(e);
                    }
                    (st.ino, st.size.min(u32::MAX as u64) as u32)
                }
                Fs::Fat32 => {
                    let mut cluster = 0u32;
                    let mut sz = 0u32;
                    if let Err(e) = fat32::lookup(name, &mut cluster, &mut sz) {
                        let _ = fd_clear(idx);
                        return Err(e);
                    }
                    (cluster, sz)
                }
                _ => {
                    let _ = fd_clear(idx);
                    return Err(Errno::Inval);
                }
            }
        }
    };

    fd_set(idx, ino, size, fs, 0);
    let fd = fd_get(idx);
    Ok(fd_token(idx, fd.epoch))
}

pub unsafe fn close(token: FdToken) -> KResult<()> {
    let idx = fd_check(token)?;
    fd_clear(idx);
    Ok(())
}

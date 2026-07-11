use super::{
    alloc_fd, fd_check, fd_clear, fd_get, fd_set, fd_token, fd_update_pos, FdToken, Fs, G_ROOT_FS,
    PERM_READ, PERM_SEEK, PERM_WRITE, VFS_MAX_FDS,
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
    let (ino, size) = match fs {
        Fs::Proc => {
            let ino = procfs::lookup(subpath)?;
            let st = procfs::stat(ino)?;
            (ino, st.size)
        }
        Fs::Ipc => {
            let ino = ipcfs::lookup(subpath)?;
            let st = ipcfs::stat(ino)?;
            (ino, st.size)
        }
        Fs::Devfs => {
            let ino = devfs::lookup(subpath)?;
            let st = devfs::stat(ino)?;
            (ino, st.size)
        }
        _ => {
            let mut st = onyxfs::OnyfsStat::default();
            match *(&raw const G_ROOT_FS) {
                Fs::Onyx => {
                    onyxfs::lookup(name, &mut st)?;
                    (st.ino, st.size.min(u32::MAX as u64) as u32)
                }
                Fs::Fat32 => {
                    let mut cluster = 0u32;
                    let mut sz = 0u32;
                    fat32::lookup(name, &mut cluster, &mut sz)?;
                    (cluster, sz)
                }
                _ => return Err(Errno::Inval),
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

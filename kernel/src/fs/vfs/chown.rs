use super::{fd_check, fd_get, resolve_mount, FdToken, Fs};
use crate::fs::onyxfs;
use onyx_core::errno::{Errno, KResult};

pub unsafe fn chown(path: &[u8], uid: u32, gid: u32) -> KResult<()> {
    if path.is_empty() || path[0] != b'/' {
        return Err(Errno::Inval);
    }
    let name = &path[1..];
    let (fs, _) = resolve_mount(name);
    if fs != Fs::Onyx {
        return Err(Errno::NoSys);
    }
    let mut st = onyxfs::OnyfsStat::default();
    let ino = onyxfs::lookup(name, &mut st)?;
    onyxfs::set_uid_gid(ino, uid, gid)
}

pub unsafe fn fchown(token: FdToken, uid: u32, gid: u32) -> KResult<()> {
    let idx = fd_check(token)?;
    let fd = fd_get(idx);
    if fd.fs != Fs::Onyx {
        return Err(Errno::NoSys);
    }
    onyxfs::set_uid_gid(fd.ino, uid, gid)
}

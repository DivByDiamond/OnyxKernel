use super::{FdToken, Fs, fd_check, fd_get, resolve_mount};
use crate::fs::onyxfs;
use onyx_core::errno::{Errno, KResult};

pub unsafe fn chmod(path: &[u8], mode: u32) -> KResult<()> {
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
    onyxfs::set_mode(ino, mode)
}

pub unsafe fn fchmod(token: FdToken, mode: u32) -> KResult<()> {
    let idx = fd_check(token)?;
    let fd = fd_get(idx);
    if fd.fs != Fs::Onyx {
        return Err(Errno::NoSys);
    }
    onyxfs::set_mode(fd.ino, mode)
}

use super::{FdToken, Fs, fd_check, fd_get, is_kernel_boot, resolve_mount};
use crate::fs::onyxfs;
use onyx_core::errno::{Errno, KResult};

pub unsafe fn chmod(path: &[u8], mode: u32) -> KResult<()> {
    unsafe {
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
        if !is_kernel_boot() {
            let cur = crate::proc::current();
            if cur.uid != 0 && cur.uid != st.uid {
                return Err(Errno::Perm);
            }
        }
        onyxfs::set_mode(ino, mode)
    }
}

pub unsafe fn fchmod(token: FdToken, mode: u32) -> KResult<()> {
    unsafe {
        let idx = fd_check(token)?;
        let fd = fd_get(idx);
        if fd.fs != Fs::Onyx {
            return Err(Errno::NoSys);
        }
        if !is_kernel_boot() {
            let mut st = onyxfs::OnyfsStat::default();
            let _ = onyxfs::stat(fd.ino, &mut st);
            let cur = crate::proc::current();
            if cur.uid != 0 && cur.uid != st.uid {
                return Err(Errno::Perm);
            }
        }
        onyxfs::set_mode(fd.ino, mode)
    }
}

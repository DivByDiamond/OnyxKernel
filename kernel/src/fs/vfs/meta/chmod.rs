use crate::fs::onyxfs;
use crate::fs::vfs::{FdToken, Fs, fd_check, fd_get, is_kernel_boot, resolve_mount};
use onyx_core::errno::{Errno, KResult};

/// # Safety
///
/// Caller contract: path comes from the syscall layer's parse_user_path
/// (kernel-side NUL-free slice); ownership check is performed inside.
pub unsafe fn chmod(path: &[u8], mode: u32) -> KResult<()> {
    // SAFETY: path slice is kernel-side; mount/owner checks run before
    // onyxfs::set_mode. onyxfs takes no cross-hart lock (journal = crash
    // recovery only); concurrent chmod calls are not serialized — caller
    // contract: do not race metadata updates across harts.
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

/// # Safety
///
/// Caller contract: token must be a live fd token of the calling context;
/// ownership check is performed inside.
pub unsafe fn fchmod(token: FdToken, mode: u32) -> KResult<()> {
    // SAFETY: fd_check validates idx and epoch. onyxfs::stat/set_mode take
    // no cross-hart lock — concurrent fchmod calls are not serialized
    // (journal is crash recovery only); caller must not race across harts.
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

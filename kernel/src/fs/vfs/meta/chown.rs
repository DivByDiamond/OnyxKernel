use crate::fs::onyxfs;
use crate::fs::vfs::{FdToken, Fs, fd_check, fd_get, resolve_mount};
use onyx_core::errno::{Errno, KResult};

/// # Safety
///
/// Caller contract: path comes from the syscall layer's parse_user_path
/// (kernel-side slice). NOTE: neither this helper nor sys_chown performs an
/// ownership/privilege check (unlike chmod) - any process can chown.
pub unsafe fn chown(path: &[u8], uid: u32, gid: u32) -> KResult<()> {
    // SAFETY: path slice is kernel-side; onyxfs::lookup/set_uid_gid take
    // no cross-hart lock — concurrent chown calls are not serialized
    // (journal is crash recovery only); caller must not race across harts.
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
        onyxfs::set_uid_gid(ino, uid, gid)
    }
}

/// # Safety
///
/// Caller contract: token must be a live fd token of the calling context.
/// NOTE: like chown, no ownership check is performed here.
pub unsafe fn fchown(token: FdToken, uid: u32, gid: u32) -> KResult<()> {
    // SAFETY: fd_check validates idx and epoch. onyxfs::set_uid_gid takes
    // no cross-hart lock — concurrent fchown calls are not serialized;
    // caller must not race metadata updates across harts.
    unsafe {
        let idx = fd_check(token)?;
        let fd = fd_get(idx);
        if fd.fs != Fs::Onyx {
            return Err(Errno::NoSys);
        }
        onyxfs::set_uid_gid(fd.ino, uid, gid)
    }
}

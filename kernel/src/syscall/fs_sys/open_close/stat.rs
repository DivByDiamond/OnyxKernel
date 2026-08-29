use crate::fs::vfs;
use crate::syscall::handler::user_ptr_ok;
use onyx_core::errno::Errno;

#[repr(C)]
pub struct UserStat {
    pub st_dev: u64,
    pub st_ino: u64,
    pub st_mode: u32,
    pub st_nlink: u32,
    pub st_uid: u32,
    pub st_gid: u32,
    pub __pad0: u32,
    pub st_rdev: u64,
    pub st_size: i64,
    pub st_blksize: i64,
    pub st_blocks: i64,
    pub st_atime: i64,
    pub st_atime_nsec: i64,
    pub st_mtime: i64,
    pub st_mtime_nsec: i64,
    pub st_ctime: i64,
    pub st_ctime_nsec: i64,
    pub __unused: [i64; 3],
}

impl UserStat {
    pub const ZERO: UserStat = UserStat {
        st_dev: 0,
        st_ino: 0,
        st_mode: 0,
        st_nlink: 0,
        st_uid: 0,
        st_gid: 0,
        __pad0: 0,
        st_rdev: 0,
        st_size: 0,
        st_blksize: 0,
        st_blocks: 0,
        st_atime: 0,
        st_atime_nsec: 0,
        st_mtime: 0,
        st_mtime_nsec: 0,
        st_ctime: 0,
        st_ctime_nsec: 0,
        __unused: [0; 3],
    };
}

/// # Safety
///
/// Caller must be on the syscall path with a live current-process root_pa;
/// `out_va` is an untrusted user address validated here before any write.
unsafe fn fill_user_stat(
    out_va: u64,
    ino: u32,
    size: u64,
    mode: u32,
    uid: u32,
    gid: u32,
    mtime: u64,
    atime: u64,
    ctime: u64,
) -> onyx_core::errno::KResult<()> {
    // SAFETY: out_va passed user_ptr_ok (sizeof UserStat) above and
    // copy_to_user re-validates each page as a writable user mapping before
    // writing; the source is a fully initialized stack UserStat.
    unsafe {
        if !user_ptr_ok(out_va, core::mem::size_of::<UserStat>() as u64) {
            return Err(Errno::Fault);
        }
        let ifmt = if mode & 0o170000 == 0o040000 {
            0o040000
        } else {
            0o100000
        };
        let st_mode: u32 = ifmt | 0o755;
        let stat = UserStat {
            st_dev: 0,
            st_ino: ino as u64,
            st_mode,
            st_nlink: 1,
            st_uid: uid,
            st_gid: gid,
            __pad0: 0,
            st_rdev: 0,
            st_size: size as i64,
            st_blksize: 512,
            st_blocks: size.div_ceil(512) as i64,
            st_atime: atime as i64,
            st_atime_nsec: 0,
            st_mtime: mtime as i64,
            st_mtime_nsec: 0,
            st_ctime: ctime as i64,
            st_ctime_nsec: 0,
            __unused: [0; 3],
        };
        crate::mm::vmm::copy_to_user(
            crate::proc::current().root_pa,
            out_va,
            core::ptr::addr_of!(stat).cast::<u8>(),
            core::mem::size_of::<UserStat>(),
        )
    }
}

#[inline(never)]
/// # Safety
///
/// Call only from handler::handle's syscall path: current process set, ACL
/// checked; `path`/`st_buf` are validated inside before use.
pub unsafe fn sys_stat(path: u64, st_buf: u64) -> i64 {
    // SAFETY: parse_user_path validates the user path internally; st_buf
    // passed user_ptr_ok above and fill_user_stat re-validates every page
    // via copy_to_user before writing the 128-byte record.
    unsafe {
        let mut path_buf = [0u8; 256];
        let path_len = match crate::syscall::handler::parse_user_path(path, &mut path_buf) {
            Some(l) => l,
            None => return Errno::Inval.as_i64(),
        };
        let path_bytes = &path_buf[..path_len];
        if !user_ptr_ok(st_buf, core::mem::size_of::<UserStat>() as u64) {
            return Errno::Inval.as_i64();
        }
        let token = match vfs::open(path_bytes, vfs::PERM_READ) {
            Ok(t) => t,
            Err(e) => return e.as_i64(),
        };
        let mut size = 0u32;
        let res_stat = vfs::stat(token, &mut size);
        let _ = vfs::close(token);
        match res_stat {
            Ok(()) => {
                let mut st = crate::fs::onyxfs::OnyfsStat::default();
                let (mtime, atime, ctime, ino, mode, uid, gid) =
                    match crate::fs::onyxfs::lookup(path_bytes, &mut st) {
                        Ok(_) => (
                            st.mtime, st.atime, st.ctime, st.ino, st.mode, st.uid, st.gid,
                        ),
                        Err(_) => {
                            let now = crate::srv::timer::uptime_us() / 1_000_000;
                            (now, now, now, 0u32, 0u32, 0u32, 0u32)
                        }
                    };
                match fill_user_stat(
                    st_buf,
                    ino,
                    size as u64,
                    mode,
                    uid,
                    gid,
                    mtime,
                    atime,
                    ctime,
                ) {
                    Ok(()) => 0,
                    Err(e) => e.as_i64(),
                }
            }
            Err(e) => e.as_i64(),
        }
    }
}

/// # Safety
///
/// Call only from handler::handle's syscall path: current process set, ACL
/// checked; `st_buf` is validated inside before use.
pub unsafe fn sys_fstat(token: u64, st_buf: u64) -> i64 {
    // SAFETY: st_buf passed user_ptr_ok above and fill_user_stat re-validates
    // every page via copy_to_user before writing the 128-byte record.
    unsafe {
        if !user_ptr_ok(st_buf, core::mem::size_of::<UserStat>() as u64) {
            return Errno::Inval.as_i64();
        }
        let mut size = 0u32;
        match vfs::stat(token, &mut size) {
            Ok(()) => {
                let now = crate::srv::timer::uptime_us() / 1_000_000;
                match fill_user_stat(st_buf, 0, size as u64, 0, 0, 0, now, now, now) {
                    Ok(()) => 0,
                    Err(e) => e.as_i64(),
                }
            }
            Err(e) => e.as_i64(),
        }
    }
}

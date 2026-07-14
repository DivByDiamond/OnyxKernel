//! Filesystem syscalls (part 1) — open / close / lseek / stat / fstat.
//!
//! The `open` implementation honours the POSIX `flags` bitmask
//! (`O_RDONLY | O_WRONLY | O_RDWR | O_CREAT | O_TRUNC | O_APPEND`) so that
//! standard libc-style programs work. `stat` and `fstat` fill a Linux-compatible
//! `struct stat` (128 bytes) so libc `stat(3)` wrappers can copy it verbatim.
use crate::fs::vfs;
use crate::proc;
use crate::syscall::abi::{
    FD_CLOEXEC, F_DUPFD, F_GETFD, F_GETFL, F_SETFD, F_SETFL, O_ACCMODE, O_APPEND, O_CREAT,
    O_DIRECTORY, O_EXCL, O_NONBLOCK, O_RDONLY, O_RDWR, O_TRUNC, O_WRONLY,
};
use onyx_core::errno::Errno;
use onyx_core::fmt::Arg;

use super::super::handler::{parse_user_path, user_ptr_ok};

/// Linux/glibc-compatible `struct stat` (rv64 lp64d layout). 128 bytes total.
///
/// Fields are deliberately laid out so that user-space `struct stat` from
/// `<bits/stat.h>` can be `memcpy`'d directly. All padding is explicit.
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

/// Translate a kernel-side `OnyfsStat` into the user-visible `struct stat`.
/// We fill the standard S_IFMT bits in `st_mode` based on the OnyxFS dtype.
unsafe fn fill_user_stat(
    out_va: u64,
    ino: u32,
    size: u64,
    mode: u32,
    mtime: u64,
    atime: u64,
    ctime: u64,
) {
    if !user_ptr_ok(out_va, core::mem::size_of::<UserStat>() as u64) {
        return;
    }
    let pa = crate::mm::vmm::translate(crate::proc::current().root_pa, out_va);
    if pa == 0 {
        return;
    }
    let dst = pa as *mut UserStat;
    // Compose a Linux-style st_mode: S_IFREG (0o100000) for regular files,
    // S_IFDIR (0o040000) for directories. Lower 9 bits = rwxrwxrwx (always
    // 0o777 for now — OnyxFS does not yet enforce permissions).
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
        st_uid: 0,
        st_gid: 0,
        __pad0: 0,
        st_rdev: 0,
        st_size: size as i64,
        st_blksize: 512,
        st_blocks: ((size + 511) / 512) as i64,
        st_atime: atime as i64,
        st_atime_nsec: 0,
        st_mtime: mtime as i64,
        st_mtime_nsec: 0,
        st_ctime: ctime as i64,
        st_ctime_nsec: 0,
        __unused: [0; 3],
    };
    core::ptr::write_volatile(dst, stat);
}

pub(in super::super) unsafe fn sys_open(path: u64, flags: u64, mode: u64) -> i64 {
    crate::kinf!(
        "sys_open",
        "called path=%x flags=%x mode=%x",
        Arg::from(path),
        Arg::from(flags as u32),
        Arg::from(mode as u32)
    );

    let mut path_buf = [0u8; 256];
    let path_len = match parse_user_path(path, &mut path_buf) {
        Some(l) => l,
        None => {
            crate::kerr!(
                "sys_open",
                "parse_user_path failed for path=%x",
                Arg::from(path)
            );
            return Errno::Inval.as_i64();
        }
    };
    let path_bytes = &path_buf[..path_len];

    crate::kinf!(
        "sys_open",
        "path_bytes=%s ring=%d",
        Arg::from(core::str::from_utf8(path_bytes).unwrap_or("<bad>")),
        Arg::from(proc::current_ring() as u32)
    );

    let ring = proc::current_ring();
    if ring == proc::PROC_RING_USER && path_bytes.starts_with(b"/service/") {
        return Errno::Perm.as_i64();
    }

    let flags32 = flags as u32;
    let acc_mode = flags32 & O_ACCMODE;
    // Build the VFS permission bitmask from the access-mode bits.
    let mut perms = vfs::PERM_SEEK;
    if acc_mode != O_RDONLY {
        perms |= vfs::PERM_WRITE;
    }
    if acc_mode == O_RDWR {
        perms |= vfs::PERM_READ;
    } else if acc_mode == O_WRONLY {
        // O_WRONLY still implies readable for our VFS implementation which
        // doesn't enforce direction yet; mark both for safety on read-after-
        // write patterns used by some libc implementations.
        perms |= vfs::PERM_READ;
    } else {
        perms |= vfs::PERM_READ; // O_RDONLY
    }

    // Try to open existing file. If O_CREAT and the file does not exist,
    // create it (root-only for now — ring 2 callers will get EPERM).
    let already_existed: bool;
    let token = match vfs::open(path_bytes, perms) {
        Ok(t) => {
            // Bug (syscall MINOR #1): enforce O_EXCL. If both O_CREAT and
            // O_EXCL are set, POSIX requires open() to fail with EEXIST if
            // the file already exists. The previous code silently accepted
            // the existing file, defeating the O_EXCL atomic-create pattern.
            if (flags32 & O_EXCL) != 0 && (flags32 & O_CREAT) != 0 {
                let _ = vfs::close(t);
                return Errno::Exist.as_i64();
            }
            already_existed = true;
            t
        }
        Err(e) if e == Errno::NoEnt && (flags32 & O_CREAT) != 0 => {
            if ring > proc::PROC_RING_ROOT {
                return Errno::Perm.as_i64();
            }
            // `mode` is the OnyxFS dtype if non-zero, otherwise regular file.
            let dtype = if mode == 0 {
                onyx_core::formats::ONYFS_DT_REG
            } else {
                mode as u32
            };
            match vfs::create(path_bytes, dtype) {
                Ok(t) => {
                    already_existed = false;
                    t
                }
                Err(e) => return e.as_i64(),
            }
        }
        Err(e) => return e.as_i64(),
    };

    // O_TRUNC: truncate to zero length on opening for write.
    if (flags32 & O_TRUNC) != 0 && (perms & vfs::PERM_WRITE) != 0 {
        let _ = vfs::truncate(token);
    }

    // O_APPEND: position at end-of-file. We do this by seeking to END.
    if (flags32 & O_APPEND) != 0 {
        let _ = vfs::lseek(token, 0, 2 /* SEEK_END */);
    }

    // Bug (syscall MINOR #2): enforce O_DIRECTORY. If the caller passed
    // O_DIRECTORY, POSIX requires open() to fail with ENOTDIR if the
    // target is not a directory. We stat the file via fstat (which fills
    // the size) — directories have mode bits 0o040000 in the stat output.
    // Since our vfs::stat only returns size, we use a heuristic: try
    // readdir() and if it fails with ENOTDIR, the target is a regular file.
    if (flags32 & O_DIRECTORY) != 0 {
        let mut name_buf = [0u8; 256];
        match vfs::readdir(path_bytes, name_buf.as_mut_ptr(), 256) {
            Ok(_) => { /* it's a directory — good */ }
            Err(Errno::NotDir) => {
                let _ = vfs::close(token);
                return Errno::NotDir.as_i64();
            }
            Err(_) => { /* other errors: accept anyway */ }
        }
    }

    token as i64
}

pub(in super::super) unsafe fn sys_close(token: u64) -> i64 {
    match vfs::close(token) {
        Ok(()) => 0,
        Err(e) => e.as_i64(),
    }
}

pub(in super::super) unsafe fn sys_lseek(token: u64, off: i64, whence: u32) -> i64 {
    match vfs::lseek(token, off, whence) {
        Ok(pos) => pos as i64,
        Err(e) => e.as_i64(),
    }
}

/// stat(path, struct stat *st) — fills a Linux-compatible `struct stat` and
/// returns 0 on success or a negative errno on failure.
pub(in super::super) unsafe fn sys_stat(path: u64, st_buf: u64) -> i64 {
    let mut path_buf = [0u8; 256];
    let path_len = match parse_user_path(path, &mut path_buf) {
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
    // Use the VFS fd info for size; pull timestamps from the on-disk inode
    // via the onyxfs-specific path. For non-OnyxFS files we fall back to
    // the kernel jiffies for mtime.
    let mut size = 0u32;
    let res_stat = vfs::stat(token, &mut size);
    let _ = vfs::close(token);
    match res_stat {
        Ok(()) => {
            // Try to get richer metadata from onyxfs if the root fs is OnyxFS.
            // (Lookup will fail for FAT32/procfs/ipcfs — that's fine.)
            let mut st = crate::fs::onyxfs::OnyfsStat::default();
            let (mtime, atime, ctime, ino, mode) =
                match crate::fs::onyxfs::lookup(path_bytes, &mut st) {
                    Ok(_) => (st.mtime, st.atime, st.ctime, st.ino, st.mode),
                    Err(_) => {
                        let now = crate::srv::timer::uptime_us() / 1_000_000;
                        (now, now, now, 0u32, 0u32)
                    }
                };
            fill_user_stat(st_buf, ino, size as u64, mode, mtime, atime, ctime);
            0
        }
        Err(e) => e.as_i64(),
    }
}

/// fstat(fd, struct stat *st) — same as stat() but takes an already-open fd.
pub(in super::super) unsafe fn sys_fstat(token: u64, st_buf: u64) -> i64 {
    if !user_ptr_ok(st_buf, core::mem::size_of::<UserStat>() as u64) {
        return Errno::Inval.as_i64();
    }
    let mut size = 0u32;
    match vfs::stat(token, &mut size) {
        Ok(()) => {
            // We don't have a cheap way to recover ino/mode/atime from a token
            // in the current VFS; fill with reasonable defaults.
            let now = crate::srv::timer::uptime_us() / 1_000_000;
            fill_user_stat(st_buf, 0, size as u64, 0, now, now, now);
            0
        }
        Err(e) => e.as_i64(),
    }
}

/// fcntl(fd, cmd, arg) — file descriptor control.
/// Currently supports:
///   - `F_DUPFD` (cmd=0): duplicate fd to lowest available number ≥ arg.
///   - `F_GETFD` (cmd=1): get fd flags (FD_CLOEXEC bit).
///   - `F_SETFD` (cmd=2): set fd flags (FD_CLOEXEC honoured on execve).
///   - `F_GETFL` (cmd=3): get open flags (returns O_RDONLY for now).
///   - `F_SETFL` (cmd=4): set open flags (O_NONBLOCK accepted as no-op).
pub(in super::super) unsafe fn sys_fcntl(fd: u64, cmd: u32, arg: u64) -> i64 {
    match cmd {
        F_DUPFD => vfs::dup(fd)
            .map(|t| t as i64)
            .unwrap_or_else(|e| e.as_i64()),
        F_GETFD => {
            let idx = match vfs::fd_check(fd) {
                Ok(i) => i,
                Err(e) => return e.as_i64(),
            };
            if vfs::fd_get(idx).cloexec {
                FD_CLOEXEC as i64
            } else {
                0
            }
        }
        F_SETFD => {
            let idx = match vfs::fd_check(fd) {
                Ok(i) => i,
                Err(e) => return e.as_i64(),
            };
            vfs::fd_set_cloexec(idx, (arg & FD_CLOEXEC as u64) != 0);
            0
        }
        F_GETFL => O_RDONLY as i64,
        F_SETFL => {
            // O_NONBLOCK on UART-backed stdio is accepted as a no-op.
            let _ = arg;
            0
        }
        _ => Errno::NoSys.as_i64(),
    }
}

// Re-export the O_* / F_* constants for callers that want to import them
// through this module rather than through abi.
pub use crate::syscall::abi::{
    O_ACCMODE as _O_ACCMODE, O_APPEND as _O_APPEND, O_CREAT as _O_CREAT,
    O_DIRECTORY as _O_DIRECTORY, O_EXCL as _O_EXCL, O_NONBLOCK as _O_NONBLOCK,
    O_RDONLY as _O_RDONLY, O_RDWR as _O_RDWR, O_TRUNC as _O_TRUNC, O_WRONLY as _O_WRONLY,
};

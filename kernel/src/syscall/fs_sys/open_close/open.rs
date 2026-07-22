use crate::fs::vfs;
use crate::proc;
use crate::syscall::abi::{
    O_ACCMODE, O_APPEND, O_CREAT, O_DIRECTORY, O_EXCL, O_RDONLY, O_RDWR, O_TRUNC, O_WRONLY,
};
use crate::syscall::handler::parse_user_path;
use onyx_core::errno::Errno;
use onyx_core::fmt::Arg;

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

    let cur = proc::current();
    let is_root = cur.uid == 0 || ring <= proc::PROC_RING_ROOT;
    if !is_root {
        let mut st = crate::fs::onyxfs::OnyfsStat::default();
        if crate::fs::onyxfs::lookup(path_bytes, &mut st).is_ok() {
            let flags32 = flags as u32;
            let acc_mode = flags32 & O_ACCMODE;
            let want_read = acc_mode == O_RDONLY || acc_mode == O_RDWR || (flags32 & O_CREAT) != 0;
            let want_write =
                acc_mode == O_WRONLY || acc_mode == O_RDWR || (flags32 & (O_TRUNC | O_APPEND)) != 0;

            let mode = st.mode;
            let owner_ok = cur.uid == st.uid;
            let group_ok = cur.gid == st.gid;
            let perm_bits = if owner_ok {
                mode & 0o700
            } else if group_ok {
                (mode >> 3) & 0o700
            } else {
                mode & 0o007
            };

            if want_read && (perm_bits & 0o400) == 0 {
                crate::kerr!(
                    "sys_open",
                    "EPERM: uid=%d path=%s mode=%o",
                    Arg::from(cur.uid),
                    Arg::from(core::str::from_utf8(path_bytes).unwrap_or("<bad>")),
                    Arg::from(mode)
                );
                return Errno::Perm.as_i64();
            }
            if want_write && (perm_bits & 0o200) == 0 {
                crate::kerr!(
                    "sys_open",
                    "EPERM: uid=%d path=%s mode=%o",
                    Arg::from(cur.uid),
                    Arg::from(core::str::from_utf8(path_bytes).unwrap_or("<bad>")),
                    Arg::from(mode)
                );
                return Errno::Perm.as_i64();
            }
        }
    }

    let flags32 = flags as u32;
    let acc_mode = flags32 & O_ACCMODE;
    let mut perms = vfs::PERM_SEEK;
    if acc_mode != O_RDONLY {
        perms |= vfs::PERM_WRITE;
    }
    if acc_mode == O_RDWR {
        perms |= vfs::PERM_READ;
    } else if acc_mode == O_WRONLY {
        perms |= vfs::PERM_READ;
    } else {
        perms |= vfs::PERM_READ;
    }

    let token = match vfs::open(path_bytes, perms) {
        Ok(t) => {
            if (flags32 & O_EXCL) != 0 && (flags32 & O_CREAT) != 0 {
                let _ = vfs::close(t);
                return Errno::Exist.as_i64();
            }
            t
        }
        Err(e) if e == Errno::NoEnt && (flags32 & O_CREAT) != 0 => {
            if ring > proc::PROC_RING_ROOT {
                return Errno::Perm.as_i64();
            }
            let dtype = if mode == 0 {
                onyx_core::formats::ONYFS_DT_REG
            } else {
                mode as u32
            };
            match vfs::create(path_bytes, dtype) {
                Ok(t) => t,
                Err(e) => return e.as_i64(),
            }
        }
        Err(e) => return e.as_i64(),
    };

    if (flags32 & O_TRUNC) != 0 && (perms & vfs::PERM_WRITE) != 0 {
        let _ = vfs::truncate(token);
    }

    if (flags32 & O_APPEND) != 0 {
        let _ = vfs::lseek(token, 0, 2);
    }

    if (flags32 & O_DIRECTORY) != 0 {
        let mut name_buf = [0u8; 256];
        match vfs::readdir(path_bytes, name_buf.as_mut_ptr(), 256) {
            Ok(_) => {}
            Err(Errno::NotDir) => {
                let _ = vfs::close(token);
                return Errno::NotDir.as_i64();
            }
            Err(_) => {}
        }
    }

    token as i64
}

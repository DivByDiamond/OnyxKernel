use super::{FdToken, Fs, fd_check, fd_get};
use crate::fs::onyxfs;
use onyx_core::errno::{Errno, KResult};

pub unsafe fn fsync(token: FdToken) -> KResult<()> {
    let idx = fd_check(token)?;
    let fd = fd_get(idx);
    match fd.fs {
        Fs::Onyx => onyxfs::fsync(fd.ino),
        _ => Err(Errno::NoSys),
    }
}

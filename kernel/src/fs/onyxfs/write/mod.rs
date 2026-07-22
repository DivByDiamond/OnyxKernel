use super::{journal, G_VERSION, ONYFS_V1};
use onyx_core::errno::{Errno, KResult};

pub unsafe fn fsync(_ino: u32) -> KResult<()> {
    journal::journal_commit()
}

mod io;
mod meta;

pub use io::*;
pub use meta::*;

pub(super) fn check_v2() -> KResult<()> {
    unsafe {
        if *(&raw const G_VERSION) == ONYFS_V1 {
            return Err(Errno::NoSys);
        }
    }
    Ok(())
}

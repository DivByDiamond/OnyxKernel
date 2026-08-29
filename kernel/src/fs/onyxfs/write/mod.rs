use super::{G_VERSION, ONYFS_V1, journal};
use onyx_core::errno::{Errno, KResult};

/// # Safety
///
/// Caller must not invoke onyxfs operations concurrently from multiple harts:
/// journal_commit mutates the shared G_JOURNAL_HEAD global without a lock.
pub unsafe fn fsync(_ino: u32) -> KResult<()> {
    // SAFETY: exclusion contract documented above; journal_commit only touches
    // the module globals covered by that contract.
    unsafe { journal::journal_commit() }
}

mod io;
mod meta;

pub use io::*;
pub use meta::*;

pub(super) fn check_v2() -> KResult<()> {
    // SAFETY: reads only the G_VERSION global set by mount(); no pointers.
    unsafe {
        if G_VERSION == ONYFS_V1 {
            return Err(Errno::NoSys);
        }
    }
    Ok(())
}

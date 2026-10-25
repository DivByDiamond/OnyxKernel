//! The driver quarantines a volume after rollback has started but I/O failed.
//! Its mount context retains this gate until a successful recovery; callers
//! must not use a partially restored inode/bitmap set as a live filesystem.
use onyx_core::errno::{Errno, KResult};
pub(super) fn access(quarantined: bool) -> KResult<()> {
    if quarantined { Err(Errno::Io) } else { Ok(()) }
}

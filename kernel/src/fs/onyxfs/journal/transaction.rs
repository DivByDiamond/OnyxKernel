//! One VFS operation owns one metadata transaction, including nested helpers.
use super::super::{G_JOURNAL_HEAD, G_SNAPSHOT_QUARANTINED};
use onyx_core::errno::{Errno, KResult};

static mut DEFERRED: bool = false;
static mut ABORT: Option<Errno> = None;

/// # Safety
/// Caller holds FS_LOCK or boot exclusion around all journal calls.
pub(super) unsafe fn deferred() -> bool {
    // SAFETY: FS_LOCK serializes the operation-local state.
    unsafe { DEFERRED }
}

/// # Safety
/// Caller holds FS_LOCK and the desired Onyx mount context. `f` must not
/// recursively enter VFS or bypass journal logging for metadata writes.
pub(crate) unsafe fn transaction<T>(f: impl FnOnce() -> KResult<T>) -> KResult<T> {
    // SAFETY: FS_LOCK serializes DEFERRED and mounted globals throughout f.
    unsafe {
        super::super::snapshot::access::access(G_SNAPSHOT_QUARANTINED)?;
        if DEFERRED {
            let result = f();
            if let Err(error) = &result {
                ABORT = Some(*error);
            }
            return result;
        }
        ABORT = None;
        DEFERRED = true;
        let result = f();
        DEFERRED = false;
        let result = if let Some(error) = ABORT {
            Err(error)
        } else {
            result
        };
        ABORT = None;
        match result {
            Ok(value) => {
                super::journal_commit()?;
                Ok(value)
            }
            Err(error) => {
                // Even ENOSPC after a successful allocation must roll back;
                // a failed recovery quarantines this mount until reboot.
                if G_JOURNAL_HEAD != 0 && super::journal_recover().is_ok() {
                    G_SNAPSHOT_QUARANTINED = false;
                }
                Err(error)
            }
        }
    }
}

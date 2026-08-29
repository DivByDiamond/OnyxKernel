//! Snapshot syscalls (root-only). These delegate to the OnyxFS snapshot
//! subsystem. The ACL layer in `handler::syscall_allowed` already enforces
//! that only ring ≤ PROC_RING_ROOT may invoke them.
use crate::fs::onyxfs;
use onyx_core::errno::Errno;

use super::handler::user_ptr_ok;

/// SYS_snapshot_create(name): create a filesystem snapshot.
/// `name` is a NUL-terminated user pointer to the snapshot name.
/// # Safety
///
/// Call only from the syscall path with a current process set; `name` is
/// validated inside before use.
pub(super) unsafe fn sys_snapshot_create(name: u64) -> i64 {
    // SAFETY: the 32-byte name range passed user_ptr_ok and per-page
    // check_user_range (readable user pages) above, so the NUL scan and
    // slice construction only read mapped user memory.
    unsafe {
        if !user_ptr_ok(name, 1)
            || crate::mm::vmm::check_user_range(crate::proc::current().root_pa, name, 32, false)
                .is_err()
        {
            return Errno::Fault.as_i64();
        }
        let mut len = 0usize;
        let p = name as *const u8;
        while *p.add(len) != 0 && len < 32 {
            len += 1;
        }
        let name_bytes = core::slice::from_raw_parts(p, len);
        match onyxfs::snapshot_create(name_bytes) {
            Ok(id) => id as i64,
            Err(e) => e.as_i64(),
        }
    }
}

/// SYS_snapshot_rollback(id): restore filesystem state from snapshot `id`.
/// # Safety
///
/// Call only from the syscall path (ACL already restricts this to ring <=
/// PROC_RING_ROOT); no user memory is touched.
pub(super) unsafe fn sys_snapshot_rollback(id: u32) -> i64 {
    // SAFETY: body performs no unsafe operations; the block only wraps the
    // call to onyxfs::snapshot_rollback for the unsafe-fn dispatch convention.
    unsafe {
        match onyxfs::snapshot_rollback(id) {
            Ok(()) => 0,
            Err(e) => e.as_i64(),
        }
    }
}

/// SYS_snapshot_list(buf, len): list snapshot names into `buf`.
/// Returns the number of snapshots listed.
/// # Safety
///
/// Call only from the syscall path with a current process set; `buf`/`len`
/// are validated inside before any write.
pub(super) unsafe fn sys_snapshot_list(buf: u64, len: u64) -> i64 {
    // SAFETY: buf/len passed user_ptr_ok and the per-page writable
    // check_user_range above, so onyxfs::snapshot_list writes only mapped
    // user pages.
    unsafe {
        if len == 0 {
            return 0;
        }
        if !user_ptr_ok(buf, len) {
            return Errno::Inval.as_i64();
        }
        // The list is written through the raw user pointer inside onyxfs —
        // verify every covered page first so a bad buffer yields EFAULT.
        if crate::mm::vmm::check_user_range(crate::proc::current().root_pa, buf, len, true).is_err()
        {
            return Errno::Fault.as_i64();
        }
        match onyxfs::snapshot_list(buf as *mut u8, len as usize) {
            Ok(count) => count as i64,
            Err(e) => e.as_i64(),
        }
    }
}

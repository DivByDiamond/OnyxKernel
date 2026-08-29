//! Ring-transition syscalls — `sys_getring` and `sys_dropring`.
use crate::proc;
use onyx_core::errno::Errno;

/// SYS_getring: return current process ring (0/1/2).
/// # Safety
///
/// Call only from the syscall path with this hart's current-process slot
/// (G_HART_CURRENT) initialized; reads no user memory.
pub(super) unsafe fn sys_getring() -> i64 {
    proc::current_ring() as i64
}

/// SYS_dropping: drop to less privileged ring (one-way, never raises).
/// # Safety
///
/// Call only from the syscall path with a current process set; the ring
/// field is mutated only by the owning process's own context.
pub(super) unsafe fn sys_dropring(target: u8) -> i64 {
    // SAFETY: proc::current() reads this hart's G_HART_CURRENT slot, which
    // the trap path set to the running process; only the ring field of the
    // caller's own process is written.
    unsafe {
        let p = proc::current();
        if target < p.ring {
            return Errno::Perm.as_i64();
        } // cannot raise
        if target == p.ring {
            return 0;
        }
        p.ring = target;
        0
    }
}

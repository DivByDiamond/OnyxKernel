//! Power-management syscalls — `sys_reboot` (SBI SRST with QEMU-finisher
//! fallback, see `klog::reset_machine`). Root-only in the ACL (ring <= 1 or
//! uid 0); never returns on success.

use onyx_core::errno::Errno;

/// reboot(2)-style commands (subset of Linux's magic constants).
const CMD_HALT: u64 = 0;
const CMD_RESTART: u64 = 1;

/// # Safety
///
/// Call only from the syscall path with this hart's current-process slot
/// set; no user memory is touched. On success the machine resets and this
/// never returns.
pub(super) unsafe fn sys_reboot(cmd: u64) -> i64 {
    match cmd {
        CMD_HALT => crate::srv::klog::reset_machine(false),
        CMD_RESTART => crate::srv::klog::reset_machine(true),
        _ => Errno::Inval.as_i64(),
    }
}

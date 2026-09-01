use crate::proc;
use onyx_core::errno::Errno;

use super::super::handler::user_ptr_ok;

/// # Safety
///
/// Call only from the syscall path with this hart's current process set;
/// only the caller's own uid field is read.
pub unsafe fn sys_getuid() -> i64 {
    // SAFETY: proc::current() reads this hart's G_HART_CURRENT slot, which
    // the trap path set to the running process.
    unsafe {
        let p = proc::current();
        p.uid as i64
    }
}

/// # Safety
///
/// Call only from the syscall path with this hart's current process set;
/// only the caller's own gid field is read.
pub unsafe fn sys_getgid() -> i64 {
    // SAFETY: proc::current() reads this hart's G_HART_CURRENT slot, which
    // the trap path set to the running process.
    unsafe {
        let p = proc::current();
        p.gid as i64
    }
}

/// # Safety
///
/// Call only from the syscall path with a current process set; `buf` is
/// validated inside before any write.
pub unsafe fn sys_uname(buf: u64) -> i64 {
    // SAFETY: buf passed user_ptr_ok (390 bytes) above, the utsname is
    // staged in a kernel stack array, and copy_to_user re-validates each
    // page as a writable user mapping before writing.
    unsafe {
        if !user_ptr_ok(buf, 390) {
            return Errno::Fault.as_i64();
        }
        let sysname = b"Onyx\0";
        let nodename = b"onyx\0";
        let release = b"0.4.0\0";
        let version = b"#1 Onyx Kernel 0.4.0 (userspace-ready)\0";
        let machine = b"riscv64\0";
        // Stage the whole utsname in kernel memory and copy it out per page —
        // the struct spans multiple 4 KiB frames' worth of offsets.
        let mut out = [0u8; 325];
        let sz = 65usize;
        out[..sysname.len()].copy_from_slice(sysname);
        out[sz..sz + nodename.len()].copy_from_slice(nodename);
        out[sz * 2..sz * 2 + release.len()].copy_from_slice(release);
        out[sz * 3..sz * 3 + version.len()].copy_from_slice(version);
        out[sz * 4..sz * 4 + machine.len()].copy_from_slice(machine);
        match crate::mm::vmm::copy_to_user(proc::current().root_pa, buf, out.as_ptr(), out.len()) {
            Ok(()) => 0,
            Err(e) => e.as_i64(),
        }
    }
}

/// setuid(uid) — set the effective user ID of the current process. Only
/// ring-1 (root) processes may change uid. Returns 0 on success.
/// # Safety
///
/// Call only from the syscall path with this hart's current process set;
/// the ring check runs before the uid field is written.
pub unsafe fn sys_setuid(uid: u64) -> i64 {
    // SAFETY: proc::current() reads this hart's G_HART_CURRENT slot, which
    // the trap path set to the running process; only that process's uid is
    // written after the root-ring check.
    unsafe {
        if proc::current_ring() > proc::PROC_RING_ROOT {
            return Errno::Perm.as_i64();
        }
        let p = proc::current();
        p.uid = uid as u32;
        0
    }
}

/// setgid(gid) — set the effective group ID. Same restriction as setuid.
/// # Safety
///
/// Call only from the syscall path with this hart's current process set;
/// the ring check runs before the gid field is written.
pub unsafe fn sys_setgid(gid: u64) -> i64 {
    // SAFETY: proc::current() reads this hart's G_HART_CURRENT slot, which
    // the trap path set to the running process; only that process's gid is
    // written after the root-ring check.
    unsafe {
        if proc::current_ring() > proc::PROC_RING_ROOT {
            return Errno::Perm.as_i64();
        }
        let p = proc::current();
        p.gid = gid as u32;
        0
    }
}

/// umask(new_mask) — set the process's file-mode creation mask, returning
/// the previous mask. Only the low 9 permission bits are meaningful;
/// callers may pass a wider value, it is masked here.
/// # Safety
///
/// Call only from the syscall path with this hart's current process set;
/// only the caller's own umask field is read/written.
pub unsafe fn sys_umask(new_mask: u64) -> i64 {
    // SAFETY: proc::current() reads this hart's G_HART_CURRENT slot, which
    // the trap path set to the running process.
    unsafe {
        let p = proc::current();
        let old = p.umask;
        p.umask = (new_mask as u32) & 0o777;
        old as i64
    }
}

/// getppid() — return parent PID of the caller. PID 1's parent is 0 (kernel).
/// # Safety
///
/// Call only from the syscall path with this hart's current process set;
/// only the caller's own parent_pid field is read.
pub unsafe fn sys_getppid() -> i64 {
    // SAFETY: proc::current() reads this hart's G_HART_CURRENT slot, which
    // the trap path set to the running process.
    unsafe {
        let p = proc::current();
        p.parent_pid as i64
    }
}

/// getpgid(pid) — return process group ID of `pid`. If `pid == 0`, returns
/// the caller's pgid. We currently treat pgid == pid (no separate pgid field
/// yet), which is sufficient for simple shells.
/// # Safety
///
/// Call only from the syscall path in kernel context; must not already hold
/// proc_list_lock (by_pid takes it internally).
pub unsafe fn sys_getpgid(pid: u64) -> i64 {
    // SAFETY: by_pid() takes proc_list_lock internally for the lookup and
    // returns a node that stays live until reaped; only the pid field of
    // the found process is read.
    unsafe {
        let target = if pid == 0 {
            proc::current_pid()
        } else {
            pid as u32
        };
        match proc::by_pid(target) {
            Some(p) => p.pid as i64, // pgid == pid for now
            None => Errno::NoEnt.as_i64(),
        }
    }
}

/// setpgid(pid, pgid) — set process group. Currently a no-op success since
/// we don't yet have a separate pgid field; shells that call it will proceed
/// without error.
/// # Safety
///
/// Call only from the syscall path; the body is a no-op that touches no
/// process state or user memory.
pub unsafe fn sys_setpgid(_pid: u64, _pgid: u64) -> i64 {
    0
}

/// setsid() — create a new session. Returns the new session ID (= caller's
/// pid). For now we just return the pid; session leadership is not tracked.
/// # Safety
///
/// Call only from the syscall path with this hart's current-process slot
/// (G_HART_CURRENT) initialized; no user memory is touched.
pub unsafe fn sys_setsid() -> i64 {
    proc::current_pid() as i64
}

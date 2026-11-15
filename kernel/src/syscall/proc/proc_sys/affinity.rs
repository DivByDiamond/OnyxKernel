//! Process affinity access serialized with process reaping.
use crate::proc::process::ProcState;
use onyx_core::errno::Errno;

/// # Safety
///
/// Call only from the syscall path with this hart's current process set;
/// no user memory is touched.
///
/// sched_setaffinity UAF fix (todo P1 #3): the whole lookup → Exited check →
/// affinity write now runs under proc_list_lock. Previously by_pid() dropped
/// the lock before the write, so a concurrent waitpid could reap the target
/// (state Exited → kfree) between lookup and write — a use-after-free. The
/// Exited rejection additionally stops callers from pinning a zombie.
pub(in crate::syscall) unsafe fn sys_sched_setaffinity(pid: u64, cpu: i64) -> i64 {
    // SAFETY: the G_ALL_PROCS traversal (by_pid_unlocked) and the affinity
    // write both run under proc_list_lock taken here, excluding concurrent
    // reap (waitpid takes the same lock to unlink+kfree); the ring check
    // reads only this hart's own current Proc.
    unsafe {
        // Bug (syscall MINOR #12): validate pid range. The previous code did
        // `by_pid(pid as u32)` which silently truncated a u64 pid > u32::MAX
        // to a small value, potentially matching an unrelated process. We
        // now reject any pid that doesn't fit in u32.
        if pid > u32::MAX as u64 {
            return Errno::Inval.as_i64();
        }
        if cpu < -1 || cpu >= crate::proc::process::MAX_HARTS as i64 {
            return Errno::Inval.as_i64();
        }
        let cur = crate::proc::current();
        crate::proc::process::proc_list_lock();
        let target = crate::proc::process::by_pid_unlocked(pid as u32);
        let res = match target {
            Some(p) => {
                // UAF guard: an Exited (zombie) process may be reaped as soon
                // as the lock is dropped — refuse it outright (ESRCH).
                if matches!(p.state, ProcState::Exited) {
                    Err(Errno::NoEnt)
                } else if cur.pid != pid as u32 && cur.ring > crate::proc::PROC_RING_ROOT {
                    Err(Errno::Perm)
                } else {
                    p.affinity = cpu as i32;
                    Ok(())
                }
            }
            None => Err(Errno::NoEnt),
        };
        crate::proc::process::proc_list_unlock();
        match res {
            Ok(()) => 0,
            Err(e) => e.as_i64(),
        }
    }
}

/// # Safety
///
/// Call only from the syscall path with this hart's current process set;
/// no user memory is touched. Same proc_list_lock discipline as
/// sys_sched_setaffinity (the read would otherwise race a concurrent reap).
pub(in crate::syscall) unsafe fn sys_sched_getaffinity(pid: u64) -> i64 {
    // SAFETY: traversal + affinity read under proc_list_lock taken here.
    unsafe {
        if pid > u32::MAX as u64 {
            return Errno::Inval.as_i64();
        }
        crate::proc::process::proc_list_lock();
        let res = match crate::proc::process::by_pid_unlocked(pid as u32) {
            Some(p) => {
                if matches!(p.state, ProcState::Exited) {
                    Errno::NoEnt.as_i64()
                } else {
                    p.affinity as i64
                }
            }
            None => Errno::NoEnt.as_i64(),
        };
        crate::proc::process::proc_list_unlock();
        res
    }
}

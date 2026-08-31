//! Process-management syscalls — `sys_exit`, `sys_yield`, `sys_getpid`,
//! `sys_spawn`, `sys_wait`, `sys_kill`, `sys_sigmask`.
use crate::arch::trap_frame::TrapFrame;
use crate::proc;
use crate::proc::process::ProcState;
use onyx_core::errno::Errno;

use super::handler::user_ptr_ok;

/// # Safety
///
/// Call only from the syscall path with this hart's current-process slot
/// (G_HART_CURRENT) initialized; no user memory is touched.
pub(super) unsafe fn sys_exit(code: u64) -> i64 {
    // SAFETY: body performs no unsafe operations; the block only wraps the
    // safe proc::current_pid/exit calls for the unsafe-fn dispatch convention.
    unsafe {
        let pid = proc::current_pid();
        proc::exit(pid, code as i32);
        0
    }
}

/// # Safety
///
/// Call only from the syscall path with this hart's current-process slot
/// (G_HART_CURRENT) initialized; no user memory is touched.
pub(super) unsafe fn sys_yield() -> i64 {
    // SAFETY: body performs no unsafe operations; the block only wraps the
    // safe set_need_resched/hart_id calls for the unsafe-fn dispatch
    // convention.
    unsafe {
        proc::set_need_resched(proc::hart_id(), true);
        0
    }
}
/// # Safety
///
/// Call only from the syscall path with this hart's current-process slot
/// (G_HART_CURRENT) initialized; no user memory is touched.
pub(super) unsafe fn sys_getpid() -> i64 {
    proc::current_pid() as i64
}

/// SYS_spawn: create new process from .onx file.
/// # Safety
///
/// Call only from the syscall path with a current process set and a live
/// trap frame; `path`/`argv` are validated inside before use.
pub(super) unsafe fn sys_spawn(_tf: &mut TrapFrame, path: u64, argv: u64, ring_hint: u8) -> i64 {
    // SAFETY: the 256-byte path range passed user_ptr_ok and per-page
    // check_user_range (readable user pages) above, so the NUL scan and
    // slice only read mapped user memory.
    unsafe {
        if !user_ptr_ok(path, 1)
            || crate::mm::vmm::check_user_range(proc::current().root_pa, path, 256, false).is_err()
        {
            return Errno::Fault.as_i64();
        }
        let mut len = 0usize;
        let p = path as *const u8;
        while *p.add(len) != 0 && len < 256 {
            len += 1;
        }
        let path_bytes = core::slice::from_raw_parts(p, len);
        let parent_pid = proc::current_pid();
        match proc::spawn(path_bytes, argv, ring_hint, parent_pid) {
            Ok(pid) => pid as i64,
            Err(e) => e.as_i64(),
        }
    }
}

/// SYS_wait: wait for child exit. Blocks (yields) until a child exits.
/// # Safety
///
/// Call only from the syscall path with a current process set and a live
/// trap frame; `status_out` is validated inside before any store.
pub(super) unsafe fn sys_wait(tf: &mut TrapFrame, status_out: u64) -> i64 {
    // SAFETY: the 4-byte status_out range passed user_ptr_ok and per-page
    // check_user_range (writable user pages) above, so proc::wait stores
    // only through mapped user memory (or a null pointer when status_out
    // is 0).
    unsafe {
        // Validate the full user range (mapping + PTE_U) up front so proc::wait
        // never stores through a bad pointer; a broken buffer returns EFAULT.
        let status_ptr = if status_out != 0 {
            if !user_ptr_ok(status_out, 4)
                || crate::mm::vmm::check_user_range(proc::current().root_pa, status_out, 4, true)
                    .is_err()
            {
                return Errno::Fault.as_i64();
            }
            status_out as *mut i32
        } else {
            core::ptr::null_mut()
        };
        match proc::wait(tf, status_ptr) {
            Ok(pid) => pid as i64,
            Err(e) => e.as_i64(),
        }
    }
}
// ── Signal syscalls ─────────────────────────────────────────────────────

/// SYS_kill(pid, signal): deliver `signal` to process `pid`. ACL allows
/// every ring (todo P2 #5); the policy lives here: ring <= ROOT signals
/// anything, ring 2 only its own group (pgid == pid) — so raise()/abort()
/// work while cross-pid signaling stays blocked.
/// # Safety
///
/// Call only from the syscall path in kernel context; must not already hold
/// proc_list_lock (signal_send takes it internally via by_pid).
pub(super) unsafe fn sys_kill(pid: u32, signal: u32) -> i64 {
    // SAFETY: signal_send's own contract (syscall/trap context, no
    // proc_list_lock held) is satisfied here; it validates pid/signal
    // internally and locks the process list for the lookup.
    unsafe {
        if proc::current_ring() > proc::PROC_RING_ROOT && pid != proc::current_pid() {
            return Errno::Perm.as_i64();
        }
        match proc::signal_send(pid, signal) {
            Ok(()) => 0,
            Err(e) => e.as_i64(),
        }
    }
}

/// SYS_sigmask(how, sig): block / unblock / set the signal mask for one
/// signal. `how`: 0 = block, 1 = unblock, 2 = set mask to just `sig`.
/// Signal 9 (KILL) cannot be blocked — `how == 0` on signal 9 is a no-op.
/// # Safety
///
/// Call only from the syscall path with this hart's current process set;
/// the mask field is mutated only by the owning process's own context.
pub(super) unsafe fn sys_sigmask(how: u32, sig: u32) -> i64 {
    // SAFETY: proc::current() reads this hart's G_HART_CURRENT slot, which
    // the trap path set to the running process; only that process's
    // signal_mask is written.
    unsafe {
        if sig >= 32 {
            return Errno::Inval.as_i64();
        }
        let p = proc::current();
        // Bug (proc SERIOUS #2): also protect SIGSTOP from being blocked.
        // The previous code only protected SIG_KILL — a process could block
        // SIGSTOP, defeating the "cannot be caught or blocked" POSIX rule.
        if (sig == proc::SIG_KILL || sig == proc::SIG_STOP) && how == 0 {
            return 0; // silently ignore attempts to block KILL/STOP
        }
        match how {
            0 => {
                // Block — but KILL/STOP cannot be blocked (checked above).
                p.signal_mask |= 1u32 << sig;
            }
            1 => {
                p.signal_mask &= !(1u32 << sig);
            }
            2 => {
                // Set mask to exactly {sig} (KILL/STOP still never blocked).
                let mut m = 0u32;
                if sig != proc::SIG_KILL && sig != proc::SIG_STOP {
                    m = 1u32 << sig;
                }
                p.signal_mask = m;
            }
            _ => return Errno::Inval.as_i64(),
        }
        0
    }
}

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
pub(super) unsafe fn sys_sched_setaffinity(pid: u64, cpu: i64) -> i64 {
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
pub(super) unsafe fn sys_sched_getaffinity(pid: u64) -> i64 {
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

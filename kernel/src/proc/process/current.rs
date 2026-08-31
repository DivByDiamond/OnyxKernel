use super::{
    globals::{G_ALL_PROCS, G_HART_CURRENT, hart_id},
    types::{PROC_RING_KERNEL, Proc, ProcState},
};

pub fn current_pid() -> u32 {
    // SAFETY: reads this hart's own G_HART_CURRENT slot (written only by the
    // owning hart's scheduler); non-null points at a live heap-allocated Proc.
    unsafe {
        let p = G_HART_CURRENT[hart_id()];
        if p.is_null() {
            return 0;
        }
        (*p).pid
    }
}

pub fn current_ring() -> u8 {
    // SAFETY: same discipline as current_pid (own-hart slot, live Proc).
    unsafe {
        let p = G_HART_CURRENT[hart_id()];
        if p.is_null() {
            return PROC_RING_KERNEL;
        }
        (*p).ring
    }
}

pub fn current_opt() -> Option<&'static mut Proc> {
    // SAFETY: own-hart slot; Some(&mut) is handed out only while the process
    // is current on this hart and not yet freed by the reaper.
    unsafe {
        let p = G_HART_CURRENT[hart_id()];
        if p.is_null() { None } else { Some(&mut *p) }
    }
}

/// # Safety
///
/// Caller contract: must run in process context on this hart (current is
/// non-null); returns a reference to the per-hart current Proc, valid only
/// while it stays current on this hart.
pub unsafe fn current() -> &'static mut Proc {
    // SAFETY: caller contract guarantees the slot is non-null and points at
    // a live heap-allocated Proc that stays current on this hart.
    unsafe {
        let p = G_HART_CURRENT[hart_id()];
        &mut *p
    }
}

/// # Safety
///
/// Caller contract: process context on this hart; writes only this
/// process's own fixed-size cwd buffer.
pub unsafe fn set_cwd(path: &[u8]) {
    // SAFETY: current() is valid per the caller contract; copy length is
    // clamped to the 256-byte cwd buffer (n <= 255 leaves room for NUL).
    unsafe {
        let p = current();
        let n = path.len().min(255);
        p.cwd[..n].copy_from_slice(&path[..n]);
        p.cwd[n] = 0;
        p.cwd_len = n as u16;
    }
}

pub fn cwd() -> &'static [u8] {
    // SAFETY: same as current_pid; slice is bounded by cwd_len <= 255 inside
    // the fixed 256-byte cwd array.
    unsafe {
        let p = current();
        &p.cwd[..p.cwd_len as usize]
    }
}

/// Find a live (non-`Free`) process by pid.
///
/// Takes `proc_list_lock` internally, so it is safe to call from any context
/// that does NOT already hold that lock (audited: all syscall/trap/signal
/// call sites qualify). The returned reference is only protected during the
/// traversal — mutate the fields per each caller's own locking discipline as
/// before.
///
/// callers already holding `proc_list_lock` must use [`by_pid_unlocked`].
///
/// # Safety
///
/// Caller contract: must NOT already hold `proc_list_lock` (self-deadlock);
/// the returned reference stays valid until the process is reaped (kfree
/// under proc_list_lock in wait()).
pub unsafe fn by_pid(pid: u32) -> Option<&'static mut Proc> {
    // SAFETY: list traversal happens entirely under proc_list_lock taken
    // here; nodes are unlinked/freed only while holding the same lock.
    unsafe {
        super::globals::proc_list_lock();
        let found = by_pid_unlocked(pid);
        super::globals::proc_list_unlock();
        found
    }
}

/// Unlocked variant of [`by_pid`] for contexts that already hold
/// `proc_list_lock` (e.g. the exit path's burst state update). Calling this
/// without the lock held is a data-race bug against concurrent list mutation.
///
/// # Safety
///
/// Caller contract: proc_list_lock MUST already be held; otherwise the
/// traversal races with concurrent unlink/reap of list nodes.
pub unsafe fn by_pid_unlocked(pid: u32) -> Option<&'static mut Proc> {
    // SAFETY: per the caller contract, proc_list_lock is held, excluding
    // concurrent list mutation while nodes are dereferenced.
    unsafe {
        let mut cur = G_ALL_PROCS;
        while !cur.is_null() {
            if (*cur).pid == pid && !matches!((*cur).state, ProcState::Free) {
                return Some(&mut *cur);
            }
            cur = (*cur).all_next;
        }
        None
    }
}

pub fn dump_all<W: onyx_core::fmt::Write>(w: &mut W) {
    // Waitpid-race follow-up (todo P1 #2): the traversal now runs under
    // proc_list_lock so it cannot race concurrent unlink/reap. try_lock
    // (not lock) because dump_all is also invoked from the panic path
    // (srv::klog), which may fire while the lock is already held — then we
    // degrade to a best-effort unlocked read instead of deadlocking the
    // panic handler.
    let locked = super::globals::G_PROC_LIST_LOCK.try_lock();
    // SAFETY: with the lock held the list cannot be mutated under us; in the
    // degraded panic path we accept a best-effort racy read (diagnostics).
    unsafe {
        let mut cur = G_ALL_PROCS;
        while !cur.is_null() {
            if !matches!((*cur).state, ProcState::Free) {
                let state_str = match (*cur).state {
                    ProcState::Ready => "Ready",
                    ProcState::Running => "Running",
                    ProcState::Exited => "Exited",
                    ProcState::Waiting => "Waiting",
                    ProcState::Creating => "Creating",
                    ProcState::Stopped => "Stopped",
                    ProcState::Free => "Free",
                };
                let args: &[onyx_core::fmt::Arg] = &[
                    onyx_core::fmt::Arg::from((*cur).pid),
                    onyx_core::fmt::Arg::from(state_str),
                    onyx_core::fmt::Arg::from((*cur).ring as u32),
                    onyx_core::fmt::Arg::from((*cur).parent_pid),
                ];
                onyx_core::fmt::vformat(w, "    pid=%d state=%s ring=%d ppid=%d\n", args);
            }
            cur = (*cur).all_next;
        }
    }
    if locked {
        super::globals::proc_list_unlock();
    }
}

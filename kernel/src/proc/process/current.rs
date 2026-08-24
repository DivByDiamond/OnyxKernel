use super::{
    globals::{G_ALL_PROCS, G_HART_CURRENT, hart_id},
    types::{PROC_RING_KERNEL, Proc, ProcState},
};

pub fn current_pid() -> u32 {
    unsafe {
        let p = G_HART_CURRENT[hart_id()];
        if p.is_null() {
            return 0;
        }
        (*p).pid
    }
}

pub fn current_ring() -> u8 {
    unsafe {
        let p = G_HART_CURRENT[hart_id()];
        if p.is_null() {
            return PROC_RING_KERNEL;
        }
        (*p).ring
    }
}

pub fn current_opt() -> Option<&'static mut Proc> {
    unsafe {
        let p = G_HART_CURRENT[hart_id()];
        if p.is_null() { None } else { Some(&mut *p) }
    }
}

pub unsafe fn current() -> &'static mut Proc {
    unsafe {
        let p = G_HART_CURRENT[hart_id()];
        &mut *p
    }
}

pub unsafe fn set_cwd(path: &[u8]) {
    unsafe {
        let p = current();
        let n = path.len().min(255);
        p.cwd[..n].copy_from_slice(&path[..n]);
        p.cwd[n] = 0;
        p.cwd_len = n as u16;
    }
}

pub fn cwd() -> &'static [u8] {
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
/// Callers already holding `proc_list_lock` must use [`by_pid_unlocked`].
pub unsafe fn by_pid(pid: u32) -> Option<&'static mut Proc> {
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
pub unsafe fn by_pid_unlocked(pid: u32) -> Option<&'static mut Proc> {
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
    unsafe {
        let mut cur = G_ALL_PROCS;
        while !cur.is_null() {
            if !matches!((*cur).state, ProcState::Free) {
                let state_str = match (*cur).state {
                    ProcState::Ready => "Ready",
                    ProcState::Running => "Running",
                    ProcState::Exited => "Exited",
                    ProcState::Waiting => "Waiting",
                    _ => "???",
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
}

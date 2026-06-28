use super::globals::G_ALL_PROCS;
use super::globals::G_HART_CURRENT;
use super::globals::hart_id;
use super::types::{Proc, ProcState, PROC_RING_KERNEL};

pub fn current_pid() -> u32 {
    unsafe {
        let p = G_HART_CURRENT[hart_id()];
        if p.is_null() {
            return 0;
        }
        if matches!((*p).state, ProcState::Running) {
            (*p).pid
        } else {
            0
        }
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
        if p.is_null() {
            None
        } else {
            Some(&mut *p)
        }
    }
}

pub unsafe fn current() -> &'static mut Proc {
    let p = G_HART_CURRENT[hart_id()];
    &mut *p
}

pub unsafe fn set_cwd(path: &[u8]) {
    let p = current();
    let n = path.len().min(255);
    p.cwd[..n].copy_from_slice(&path[..n]);
    p.cwd[n] = 0;
    p.cwd_len = n as u16;
}

pub fn cwd() -> &'static [u8] {
    unsafe {
        let p = current();
        &p.cwd[..p.cwd_len as usize]
    }
}

pub unsafe fn by_pid(pid: u32) -> Option<&'static mut Proc> {
    let mut cur = G_ALL_PROCS;
    while !cur.is_null() {
        if (*cur).pid == pid && !matches!((*cur).state, ProcState::Free) {
            return Some(&mut *cur);
        }
        cur = (*cur).all_next;
    }
    None
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

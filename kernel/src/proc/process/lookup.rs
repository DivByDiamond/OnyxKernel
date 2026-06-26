use super::globals::G_PROC_LIST;
use super::types::{Proc, ProcState};

pub unsafe fn by_pid(pid: u32) -> Option<&'static mut Proc> {
    let mut cur = G_PROC_LIST;
    while !cur.is_null() {
        if (*cur).pid == pid && !matches!((*cur).state, ProcState::Free) {
            return Some(&mut *cur);
        }
        cur = (*cur).next;
    }
    None
}

pub fn dump_all<W: onyx_core::fmt::Write>(w: &mut W) {
    unsafe {
        let mut cur = G_PROC_LIST;
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
            cur = (*cur).next;
        }
    }
}

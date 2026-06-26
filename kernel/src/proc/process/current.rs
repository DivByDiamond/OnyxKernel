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

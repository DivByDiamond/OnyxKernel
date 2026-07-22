use crate::arch::trap_frame::TrapFrame;
use core::sync::atomic::Ordering;

use super::protected_mask;
use super::SIG_KILL;
use super::SIG_STOP;
use super::{by_pid, current_for_hart, hart_id, Proc, ProcState, G_NEED_RESCHED, MAX_HARTS};
use crate::proc::lifecycle::exit;
use crate::proc::scheduler::{enqueue, rq_lock, rq_unlock};

pub unsafe fn sigreturn(tf: &mut TrapFrame) {
    let p = crate::proc::current();
    if !p.in_signal_handler {
        return;
    }
    p.in_signal_handler = false;
    p.signal_mask = p.saved_mask;
    *tf = p.saved_tf;
}

pub unsafe fn signal_check(tf: &mut TrapFrame) {
    let hartid = hart_id();
    let cur = current_for_hart(hartid);
    if cur.is_null() {
        return;
    }
    let pid = (*cur).pid;

    if (*cur).pending_signals & (1u32 << SIG_KILL) != 0 {
        (*cur).pending_signals &= !(1u32 << SIG_KILL);
        exit(pid, 128 + SIG_KILL as i32);
        G_NEED_RESCHED[hartid].store(true, Ordering::Release);
        return;
    }

    if (*cur).pending_signals & (1u32 << SIG_STOP) != 0 {
        (*cur).pending_signals &= !(1u32 << SIG_STOP);
        (*cur).state = ProcState::Waiting;
        G_NEED_RESCHED[hartid].store(true, Ordering::Release);
        return;
    }

    if (*cur).in_signal_handler {
        return;
    }

    let pending = (*cur).pending_signals & !(*cur).signal_mask;
    if pending == 0 {
        return;
    }

    let mut signum = 0u32;
    for i in 1..32u32 {
        if pending & (1u32 << i) != 0 {
            signum = i;
            break;
        }
    }
    if signum == 0 {
        return;
    }
    (*cur).pending_signals &= !(1u32 << signum);

    let handler = (*cur).signal_handlers[signum as usize];
    if handler == 0 {
        exit(pid, 128 + signum as i32);
        G_NEED_RESCHED[hartid].store(true, Ordering::Release);
        return;
    }
    if handler == 1 {
        return;
    }

    (*cur).saved_tf = *tf;
    (*cur).in_signal_handler = true;
    (*cur).saved_mask = (*cur).signal_mask;
    (*cur).signal_mask |= (*cur).signal_handler_masks[signum as usize];
    (*cur).signal_mask &= !protected_mask();

    tf.sepc = handler;
    tf.a0 = signum as u64;
    let new_sp = tf.sp.wrapping_sub(256) & !15u64;
    tf.sp = new_sp;
}

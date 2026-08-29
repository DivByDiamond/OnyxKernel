use crate::arch::trap_frame::{TrapFrame, reg_truncate, reg_widen};
use core::sync::atomic::Ordering;

use super::SIG_KILL;
use super::SIG_STOP;
use super::protected_mask;
use super::{G_NEED_RESCHED, ProcState, current_for_hart, hart_id};
use crate::proc::lifecycle::exit;

/// # Safety
///
/// Caller contract: trap-return context of the process invoking sigreturn
/// on this hart; tf is that process's current user trap frame.
pub unsafe fn sigreturn(tf: &mut TrapFrame) {
    // SAFETY: p is this hart's current process (own context); saved_tf /
    // saved_mask were written by signal_check on this same hart before the
    // handler was entered, so no cross-hart race on these fields.
    unsafe {
        let p = crate::proc::current();
        if !p.in_signal_handler {
            return;
        }
        p.in_signal_handler = false;
        p.signal_mask = p.saved_mask;
        *tf = p.saved_tf;
    }
}

/// # Safety
///
/// Caller contract: trap-return path on this hart with a non-null current;
/// tf is the user trap frame about to be restored (may be retargeted to a
/// handler or cause exit()).
pub unsafe fn signal_check(tf: &mut TrapFrame) {
    // SAFETY: all dereferences are of this hart's own current Proc (never
    // freed while current); the SIG_KILL/STOP default actions and handler
    // dispatch only touch per-process state owned by this hart's context.
    unsafe {
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

        tf.sepc = reg_truncate(handler);
        tf.a0 = reg_truncate(signum as u64);
        let new_sp = reg_widen(tf.sp).wrapping_sub(256) & !15;
        tf.sp = reg_truncate(new_sp);
    }
}

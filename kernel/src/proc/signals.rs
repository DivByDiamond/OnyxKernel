use crate::arch::trap_frame::TrapFrame;
use core::sync::atomic::Ordering;
use onyx_core::errno::{Errno, KResult};

use super::lifecycle::exit;
use super::process::{by_pid, current_for_hart, hart_id, Proc, ProcState, G_NEED_RESCHED, MAX_HARTS};
use crate::proc::scheduler::{enqueue, rq_lock, rq_unlock};

/// Signal number for KILL (POSIX SIGKILL = 9). Always honored, never blocked.
pub const SIG_KILL: u32 = 9;
/// Signal number for STOP (POSIX SIGSTOP = 19). Cannot be caught or blocked.
pub const SIG_STOP: u32 = 19;

/// Bitmask of signals that can never be blocked or caught (KILL + STOP).
/// Used by sigaction / sigprocmask / signal_check to enforce POSIX.
#[inline]
fn protected_mask() -> u32 {
    (1u32 << SIG_KILL) | (1u32 << SIG_STOP)
}

pub unsafe fn signal_send(pid: u32, signal: u32) -> KResult<()> {
    if signal == 0 || signal >= 32 {
        return Err(Errno::Inval);
    }
    let p = by_pid(pid).ok_or(Errno::NoEnt)?;
    p.pending_signals |= 1u32 << signal;
    if matches!(p.state, ProcState::Waiting) {
        p.state = ProcState::Ready;
        // Bug #11 fix: acquire the caller's rq_lock before enqueue. The
        // previous code called enqueue(hart_id(), p) without any lock,
        // racing with the scheduler on the same hart and producing
        // orphaned/duplicated runqueue entries.
        let caller_hart = hart_id();
        rq_lock(caller_hart);
        enqueue(caller_hart, p as *mut Proc);
        rq_unlock(caller_hart);
    }
    Ok(())
}

pub unsafe fn sigaction(signum: u32, act_ptr: u64, oldact_ptr: u64) -> KResult<()> {
    if signum == 0 || signum >= 32 {
        return Err(Errno::Inval);
    }
    if signum == SIG_KILL || signum == SIG_STOP {
        return Err(Errno::Inval); // cannot catch KILL or STOP
    }
    let p = crate::proc::current();
    let user_root = p.root_pa;

    // Write old action if requested.
    if oldact_ptr != 0 {
        let old_pa = crate::mm::vmm::translate_user_write(user_root, oldact_ptr);
        if old_pa != 0 {
            let dst = old_pa as *mut u64;
            *dst = p.signal_handlers[signum as usize];
            *dst.add(1) = 0;
            *dst.add(2) = 0;
            *dst.add(3) = 0;
        }
    }

    // Read new action if requested.
    if act_ptr != 0 {
        let new_pa = crate::mm::vmm::translate_user(user_root, act_ptr);
        if new_pa == 0 {
            return Err(Errno::Inval);
        }
        let src = new_pa as *const u64;
        let handler = *src;
        // sa_mask bits at offset 8.
        // Bug (proc SERIOUS #1): the previous code permanently OR-ed the
        // extra mask into the process's signal_mask. POSIX semantics say
        // sa_mask is applied TRANSIENTLY during the handler execution —
        // it blocks those signals only while the handler runs, not for
        // the rest of the process's life. We now stash the per-handler
        // mask in a separate field (signal_handler_masks) and apply it
        // only when entering the handler (in signal_check) and remove it
        // on sigreturn. Without this, every sigaction() call would
        // permanently widen the blocked set, eventually blocking
        // everything.
        let extra_mask = *src.add(1) as u32;
        p.signal_handlers[signum as usize] = handler;
        p.signal_handler_masks[signum as usize] = extra_mask & !protected_mask();
    }
    Ok(())
}

pub unsafe fn sigprocmask(how: u32, set_ptr: u64, oldset_ptr: u64) -> KResult<()> {
    let p = crate::proc::current();
    let user_root = p.root_pa;

    // Save old mask.
    if oldset_ptr != 0 {
        let old_pa = crate::mm::vmm::translate_user_write(user_root, oldset_ptr);
        if old_pa != 0 {
            *(old_pa as *mut u64) = p.signal_mask as u64;
        }
    }

    if set_ptr != 0 {
        let set_pa = crate::mm::vmm::translate_user(user_root, set_ptr);
        if set_pa == 0 {
            return Err(Errno::Inval);
        }
        let new_mask = *(set_pa as *const u64) as u32;
        // KILL and STOP can never be blocked.
        let protected = (1u32 << SIG_KILL) | (1u32 << SIG_STOP);
        match how {
            0 /* SIG_BLOCK  */ => p.signal_mask |= new_mask & !protected,
            1 /* SIG_UNBLOCK*/ => p.signal_mask &= !(new_mask & !protected),
            2 /* SIG_SETMASK*/ => p.signal_mask = new_mask & !protected,
            _ => return Err(Errno::Inval),
        }
    }
    Ok(())
}

pub unsafe fn sigreturn(tf: &mut TrapFrame) {
    let p = crate::proc::current();
    if !p.in_signal_handler {
        // Spurious sigreturn — ignore.
        return;
    }
    p.in_signal_handler = false;
    // Bug (proc SERIOUS #1, cont.): remove the per-handler mask that was
    // applied when we entered the handler. signal_check saved the
    // pre-handler mask in saved_mask; restore it now so the blocked set
    // returns to its pre-handler state.
    p.signal_mask = p.saved_mask;
    // Restore the trap frame saved when we entered the handler. We can't
    // move the whole struct at once because `tf` is `&mut` borrowed by the
    // trap handler — copy field by field.
    *tf = p.saved_tf;
}

pub unsafe fn signal_check(tf: &mut TrapFrame) {
    let hartid = hart_id();
    let cur = current_for_hart(hartid);
    if cur.is_null() {
        return;
    }
    let pid = (*cur).pid;

    // KILL cannot be blocked — check it first.
    if (*cur).pending_signals & (1u32 << SIG_KILL) != 0 {
        (*cur).pending_signals &= !(1u32 << SIG_KILL);
        exit(pid, 128 + SIG_KILL as i32);
        G_NEED_RESCHED[hartid].store(true, Ordering::Release);
        return;
    }

    // Bug (proc SERIOUS #9): SIGSTOP stops the process (transitions to
    // Waiting, doesn't terminate). The previous code fell through to the
    // default-action branch below which called exit() — terminating the
    // process instead of stopping it. POSIX requires SIGSTOP to suspend
    // the process until SIGCONT is received.
    if (*cur).pending_signals & (1u32 << SIG_STOP) != 0 {
        (*cur).pending_signals &= !(1u32 << SIG_STOP);
        (*cur).state = ProcState::Waiting;
        G_NEED_RESCHED[hartid].store(true, Ordering::Release);
        return;
    }

    // Don't deliver a new signal while a handler is running (no nesting yet).
    if (*cur).in_signal_handler {
        return;
    }

    let pending = (*cur).pending_signals & !(*cur).signal_mask;
    if pending == 0 {
        return;
    }

    // Pick the lowest-numbered pending unblocked signal.
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
    // Clear the pending bit.
    (*cur).pending_signals &= !(1u32 << signum);

    let handler = (*cur).signal_handlers[signum as usize];
    if handler == 0 {
        // Default action: terminate with `128 + signum` (Linux convention).
        exit(pid, 128 + signum as i32);
        G_NEED_RESCHED[hartid].store(true, Ordering::Release);
        return;
    }
    if handler == 1 {
        // SIG_IGN: do nothing.
        return;
    }

    // Stash the original trap frame and rewrite `tf` to enter the handler.
    (*cur).saved_tf = *tf;
    (*cur).in_signal_handler = true;
    // Bug (proc SERIOUS #1, cont.): apply the per-handler mask TRANSIENTLY.
    // Save the current mask so sigreturn can restore it, then OR in the
    // handler's sa_mask (so those signals are blocked while the handler
    // runs). KILL and STOP are never blocked.
    (*cur).saved_mask = (*cur).signal_mask;
    (*cur).signal_mask |= (*cur).signal_handler_masks[signum as usize];
    (*cur).signal_mask &= !protected_mask();

    // Set up the handler call: pc=handler, a0=signum, sp stays the same.
    tf.sepc = handler;
    tf.a0 = signum as u64;
    // Reserve a small region on the user stack for the handler's own frame.
    // We decrement sp by 256 bytes (aligned to 16) to give the handler
    // scratch space without overwriting the caller's frame.
    let new_sp = tf.sp.wrapping_sub(256) & !15u64;
    tf.sp = new_sp;
}

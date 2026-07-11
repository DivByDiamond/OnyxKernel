use crate::arch::trap_frame::TrapFrame;
use core::sync::atomic::Ordering;
use onyx_core::errno::{Errno, KResult};

use super::lifecycle::exit;
use super::process::{by_pid, current_for_hart, hart_id, Proc, ProcState, G_NEED_RESCHED};
use crate::proc::scheduler::enqueue;

/// Signal number for KILL (POSIX SIGKILL = 9). Always honored, never blocked.
pub const SIG_KILL: u32 = 9;
/// Signal number for STOP (POSIX SIGSTOP = 19). Cannot be caught or blocked.
pub const SIG_STOP: u32 = 19;

pub unsafe fn signal_send(pid: u32, signal: u32) -> KResult<()> {
    if signal == 0 || signal >= 32 {
        return Err(Errno::Inval);
    }
    let p = by_pid(pid).ok_or(Errno::NoEnt)?;
    p.pending_signals |= 1u32 << signal;
    if matches!(p.state, ProcState::Waiting) {
        p.state = ProcState::Ready;
        enqueue(hart_id(), p as *mut Proc);
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
        // sa_mask bits at offset 8 — OR them into the process signal_mask so
        // those signals are blocked while the handler runs.
        let extra_mask = *src.add(1);
        p.signal_handlers[signum as usize] = handler;
        // The extra mask is applied transiently during signal delivery, not
        // permanently; for simplicity we permanently OR it in here.
        if extra_mask != 0 {
            p.signal_mask |= extra_mask as u32;
        }
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

    // Set up the handler call: pc=handler, a0=signum, sp stays the same.
    tf.sepc = handler;
    tf.a0 = signum as u64;
    // Reserve a small region on the user stack for the handler's own frame.
    // We decrement sp by 256 bytes (aligned to 16) to give the handler
    // scratch space without overwriting the caller's frame.
    let new_sp = tf.sp.wrapping_sub(256) & !15u64;
    tf.sp = new_sp;
}

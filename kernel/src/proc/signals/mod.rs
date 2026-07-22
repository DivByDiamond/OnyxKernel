use crate::arch::trap_frame::TrapFrame;
use onyx_core::errno::{Errno, KResult};

use super::process::{
    by_pid, current_for_hart, hart_id, Proc, ProcState, G_NEED_RESCHED, MAX_HARTS,
};
use crate::proc::scheduler::{enqueue, rq_lock, rq_unlock};

pub const SIG_KILL: u32 = 9;
pub const SIG_STOP: u32 = 19;

mod handler;

pub use handler::*;

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
        return Err(Errno::Inval);
    }
    let p = crate::proc::current();
    let user_root = p.root_pa;

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

    if act_ptr != 0 {
        let new_pa = crate::mm::vmm::translate_user(user_root, act_ptr);
        if new_pa == 0 {
            return Err(Errno::Inval);
        }
        let src = new_pa as *const u64;
        let handler = *src;
        let extra_mask = *src.add(1) as u32;
        p.signal_handlers[signum as usize] = handler;
        p.signal_handler_masks[signum as usize] = extra_mask & !protected_mask();
    }
    Ok(())
}

pub unsafe fn sigprocmask(how: u32, set_ptr: u64, oldset_ptr: u64) -> KResult<()> {
    let p = crate::proc::current();
    let user_root = p.root_pa;

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
        let protected = (1u32 << SIG_KILL) | (1u32 << SIG_STOP);
        match how {
            0 => p.signal_mask |= new_mask & !protected,
            1 => p.signal_mask &= !(new_mask & !protected),
            2 => p.signal_mask = new_mask & !protected,
            _ => return Err(Errno::Inval),
        }
    }
    Ok(())
}

use core::sync::atomic::{AtomicU32, Ordering};
use onyx_core::errno::{Errno, KResult};

use super::process::{G_NEED_RESCHED, Proc, ProcState, by_pid, current_for_hart, hart_id};
use crate::proc::scheduler::{enqueue, rq_lock, rq_unlock};

pub const SIGINT: u32 = 2;
pub const SIGCHLD: u32 = 17;
pub const SIGCONT: u32 = 18;
pub const SIG_KILL: u32 = 9;
pub const SIG_STOP: u32 = 19;
pub const SIG_TSTP: u32 = 20;
pub const SIGWINCH: u32 = 28;

/// SA_NOCLDWAIT — the only sa_flag the kernel tracks (todo P2 #2): set on
/// SIGCHLD, exiting children are auto-reaped and no SIGCHLD is delivered.
pub const SA_NOCLDWAIT: u32 = 0x2;

mod handler;

pub use handler::*;

/// Foreground process: the most recently created non-init process (via
/// `spawn()` or `fork()`), i.e. the process a shell would be waiting on.
/// 0 = none yet. Console Ctrl+C delivers SIGINT to this pid; if the pid is
/// no longer a live process, the byte is dropped (quiet no-op).
static G_FG_PID: AtomicU32 = AtomicU32::new(0);

pub fn set_foreground(pid: u32) {
    G_FG_PID.store(pid, Ordering::Release);
}

#[inline]
fn protected_mask() -> u32 {
    (1u32 << SIG_KILL) | (1u32 << SIG_STOP)
}

/// # Safety
///
/// Caller contract: kernel context (syscall/trap path, SIE clear); must NOT
/// already hold proc_list_lock (by_pid takes it internally); the target may
/// be on another hart; pending_signals is a single OR-set flag.
pub unsafe fn signal_send(pid: u32, signal: u32) -> KResult<()> {
    // SAFETY: by_pid() takes proc_list_lock for the lookup (globals.rs
    // contract); the returned node stays valid until reaped (kfree only
    // after unlink under that lock), so the field updates below are on a
    // live node. The wake path follows the rq_lock discipline.
    unsafe {
        if signal == 0 || signal >= 32 {
            return Err(Errno::Inval);
        }
        let p = by_pid(pid).ok_or(Errno::NoEnt)?;
        // POSIX signal exclusivity (todo P2 #3/#4): stop and continue
        // signals cancel each other's pending bits.
        if signal == SIGCONT {
            p.pending_signals &= !((1u32 << SIG_STOP) | (1u32 << SIG_TSTP));
        } else if signal == SIG_STOP || signal == SIG_TSTP {
            p.pending_signals &= !(1u32 << SIGCONT);
        }
        p.pending_signals |= 1u32 << signal;
        // Wake rules: Waiting processes wake on any signal; a Stopped
        // process only resumes on SIGCONT (job control) or dies on SIGKILL.
        let resume = match p.state {
            ProcState::Waiting => true,
            ProcState::Stopped => signal == SIGCONT || signal == SIG_KILL,
            _ => false,
        };
        if resume {
            p.state = ProcState::Ready;
            let caller_hart = hart_id();
            rq_lock(caller_hart);
            enqueue(caller_hart, p as *mut Proc);
            rq_unlock(caller_hart);
        }
        Ok(())
    }
}

/// Deliver `signal` to the foreground process (see G_FG_PID). A stale pid
/// (exited/reaped) is a quiet no-op: Ctrl+C at an idle shell prompt must not
/// kill anything. SIGINT is catchable/blockable like POSIX (only SIG_KILL and
/// SIG_STOP are protected); with no user handler installed, signal_check's
/// default path terminates the target with code 128+SIGINT.
///
/// # Safety
///
/// Caller contract: syscall/trap context with SIE clear; may be called
/// from any hart (by_pid takes proc_list_lock internally).
pub unsafe fn signal_foreground(signal: u32) -> KResult<()> {
    // SAFETY: reads G_FG_PID (atomic acquire) then delegates to
    // signal_send, whose own caller contract applies; a stale/None pid is
    // handled as a no-op above.
    unsafe {
        if signal == 0 || signal >= 32 {
            return Err(Errno::Inval);
        }
        let pid = G_FG_PID.load(Ordering::Acquire);
        if pid == 0 {
            return Ok(());
        }
        match by_pid(pid) {
            Some(p) if !matches!(p.state, ProcState::Exited) => signal_send(pid, signal),
            _ => Ok(()),
        }
    }
}

/// # Safety
///
/// Caller contract: syscall context of the calling process (current() is
/// this hart's process); act_ptr/oldact_ptr are user pointers already
/// validated as in-range by the syscall layer.
pub unsafe fn sigaction(signum: u32, act_ptr: u64, oldact_ptr: u64) -> KResult<()> {
    // SAFETY: user pointers are range-checked by the syscall layer and
    // re-translated through the caller's own root table (translate_user /
    // translate_user_write) before each write; the Proc is this hart's
    // current process, only mutated by its own context.
    unsafe {
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
                // old sa_flags: report the live SA_NOCLDWAIT state so
                // sigaction(SIGCHLD, NULL, &old) round-trips.
                *dst.add(2) = if p.no_cldwait { SA_NOCLDWAIT as u64 } else { 0 };
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
            let flags = *src.add(2) as u32;
            p.signal_handlers[signum as usize] = handler;
            p.signal_handler_masks[signum as usize] = extra_mask & !protected_mask();
            // SA_NOCLDWAIT is only meaningful on SIGCHLD (todo P2 #2):
            // exiting children are auto-reaped (no zombie) and no SIGCHLD
            // is delivered; installing any other disposition clears it.
            if signum == SIGCHLD {
                p.no_cldwait = flags & SA_NOCLDWAIT != 0;
            }
        }
        Ok(())
    }
}

/// # Safety
///
/// Caller contract: syscall context of the calling process; set_ptr /
/// oldset_ptr are user pointers validated as in-range by the syscall layer.
pub unsafe fn sigprocmask(how: u32, set_ptr: u64, oldset_ptr: u64) -> KResult<()> {
    // SAFETY: user pointers are range-checked by the syscall layer and
    // re-translated through the caller's own root table before access;
    // the Proc is this hart's own current process.
    unsafe {
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
}

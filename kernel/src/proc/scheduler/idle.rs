use core::sync::atomic::Ordering;

use crate::{
    arch::{
        csr,
        regs::{SIE_SEIE, SIE_STIE, SSTATUS_SIE},
    },
    proc::process::{G_NEED_RESCHED, current_for_hart, hart_id},
    srv::timer,
};

pub unsafe fn is_idle() -> bool {
    unsafe { current_for_hart(hart_id()).is_null() }
}

/// Enter the per-hart idle loop. Never returns.
///
/// ── SMP design (wave 2) ──────────────────────────────────────────────────
/// An idle hart is not dead weight: every timer tick delivers a trap whose
/// handler sets G_NEED_RESCHED for this hart (`sched_tick` treats a null
/// current as "wants scheduling"), and the trap epilogue calls
/// `sched_yield`, which dequeues from the local runqueue or steals from a
/// remote one and context-switches into the stolen process. When that
/// process later yields/exits, `sched_yield` restores G_HART_IDLE_TF for
/// this hart and control resumes inside the loop below, right after `wfi`.
///
/// ── SIE / spinlock invariant (audited) ───────────────────────────────────
/// `crate::sync::SpinLock` must only be held with interrupts disabled.
/// The invariant holds by construction:
///   * Hardware clears sstatus.SIE on every trap entry; `trap_return`
///     restores saved sstatus with SIE cleared, so ALL trap-handler code
///     (the only code that takes SpinLocks reachable from this loop:
///     rq_lock / proc_list_lock inside sched_yield, timer bookkeeping)
///     runs with SIE = 0.
///   * This loop is the ONE context that sets SIE, and it does so at a
///     point where no lock can be held: the body takes no locks, and any
///     interrupt taken after SIE is set immediately re-enters the
///     SIE = 0 world before running kernel code.
///
/// Returning to USER mode never relies on this loop: user trap frames
/// carry sstatus.SPIE = 1, so `sret` re-enables interrupts by hardware
/// (SPIE → SIE) exactly when entering user space.
pub unsafe fn sched_enter_idle() -> ! {
    unsafe {
        let hartid = hart_id();
        csr::write_stvec(crate::arch::asm::trap_entry as *const () as usize as u64);
        // sscratch = 0 in kernel mode (trap_entry discriminator). The idle loop
        // never enters user space, so the stack top lives only in sp.
        csr::write_sscratch(0);
        // Enable the timer + external interrupt sources once; the sources
        // stay enabled, only sstatus.SIE toggles per iteration below.
        csr::set_sie(SIE_STIE | SIE_SEIE);
        loop {
            G_NEED_RESCHED[hartid].store(false, Ordering::Release);
            // Re-arm THIS hart's timer (per-hart mtimecmp stride / SBI
            // set_timer). trap_return cleared SIE on the way back here, so
            // without this the hart would sleep forever after its first
            // tick. No spinlock is held anywhere in this body.
            timer::init_hart(hartid);
            csr::set_sstatus(SSTATUS_SIE);
            csr::wfi();
            // Woke up (timer/external interrupt): hardware cleared SIE on
            // entry; if the tick found work, sched_yield switched us into a
            // process and we never reach this line again until the process
            // switches back to G_HART_IDLE_TF[hartid].
        }
    }
}

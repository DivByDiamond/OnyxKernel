use core::sync::atomic::Ordering;

use crate::{
    arch::{
        csr,
        regs::{SIE_SEIE, SIE_STIE, SSTATUS_SIE, SSTATUS_SPP},
        trap_frame::TrapFrame,
    },
    proc::process::{
        G_HART_IDLE_TF, G_HART_IDLE_TF_VALID, G_NEED_RESCHED, current_for_hart, hart_id,
    },
    srv::timer,
};

/// # Safety
///
/// Caller contract: init() has run; reads only this hart's current slot.
pub unsafe fn is_idle() -> bool {
    // SAFETY: own-hart G_HART_CURRENT slot read (written only by the
    // owning hart's scheduler); null or a live heap-allocated Proc.
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
///
/// Seed `G_HART_IDLE_TF[hartid]` with a synthetic-but-valid resume context
/// pointing at `sched_enter_idle`, and mark it captured.
///
/// Root-cause fix (SMP crash, todo.md "Отдельный SMP-краш под -smp 2"):
/// every hart except the boot hart calls `sched_enter_idle()` directly at
/// boot (`arch/smp/secondary.rs`), so its first real trap-out of `wfi`
/// naturally populates `G_HART_IDLE_TF` via `sched_yield`'s normal capture
/// path. The boot hart never does — `srv::main::init::launch` drops
/// straight into `enter_user(1)` — so without this seed, the first time
/// `sched_yield` needed to switch the boot hart to idle (everything it ran
/// migrated away via work-stealing, or exited) it resumed
/// `TrapFrame::zero()`: sepc=0, an instant crash.
///
/// This constructs the SAME kind of frame a real trap-out would have
/// produced — sepc at `sched_enter_idle`'s entry, sp at this hart's own
/// idle-stack top, sstatus.SPP=1 (kernel-mode resume, matches
/// `arch::asm::trap_return`'s `.Lret_kernel` path), satp = whatever this
/// hart is currently running with (kernel identity mappings are present in
/// every root, see `proc::onx::load`) — and, critically, `gp`/`tp` copied
/// from the live registers rather than left zero: `tp` is this hart's
/// hand-rolled identity register (`hart_id()` reads it raw), so a
/// zero-initialized `tp` field would have `trap_return`'s register restore
/// silently zero it out on first resume, breaking `hart_id()` for the rest
/// of this hart's life — the same class of corruption the missing
/// `+reserve-x4` flag caused (see `.cargo/config.toml`).
///
/// Reuses the ordinary `sched_switch`/`trap_return` resume path (the same,
/// well-exercised mechanism used for every other process switch) instead
/// of a hand-rolled raw-asm jump — a prior attempt at that (raw `mv
/// sp/jr`) was fragile and is why this exists as the primary fix, with the
/// old raw-asm path kept in `sched_yield` only as a defensive fallback.
///
/// # Safety
///
/// Caller contract: called once, early, from this hart's own boot context,
/// before any process ever runs on it (so `G_HART_IDLE_TF[hartid]` is not
/// yet relied upon) and before interrupts are enabled.
pub unsafe fn seed_boot_hart_idle_context(hartid: usize) {
    // SAFETY: reads this hart's own live gp/tp/satp (stable, correct for
    // this hart at boot); writes only this hart's own G_HART_IDLE_TF slot,
    // which no other hart touches (per-hart ownership, same as everywhere
    // else this array is used).
    unsafe {
        // Whole body is riscv-only: taking `sched_enter_idle`'s address
        // pulls its (SBI/CSR-heavy) definition into the link even under the
        // host `cargo test` target, where it fails to link (unrelated
        // pre-existing host-target gaps in that code path, previously
        // dormant because nothing before this function ever referenced
        // `sched_enter_idle` from code reachable under `cfg(test)`).
        #[cfg(not(test))]
        {
            let gp: usize;
            let tp: usize;
            core::arch::asm!("mv {0}, gp", out(reg) gp);
            core::arch::asm!("mv {0}, tp", out(reg) tp);
            let satp = csr::read_satp();
            let stack_top = crate::arch::smp::G_SEC_STACKS.as_ptr() as usize
                + (hartid + 1) * crate::arch::smp::SEC_STACK_SIZE;
            use crate::arch::trap_frame::reg_truncate;
            let mut tf = TrapFrame::zero();
            tf.sepc = reg_truncate(sched_enter_idle as *const () as usize as u64);
            tf.sp = reg_truncate(stack_top as u64);
            tf.gp = reg_truncate(gp as u64);
            tf.tp = reg_truncate(tp as u64);
            tf.sstatus = reg_truncate(SSTATUS_SPP);
            tf.satp = reg_truncate(satp);
            G_HART_IDLE_TF[hartid] = tf;
            G_HART_IDLE_TF_VALID[hartid].store(true, Ordering::Release);
        }
        #[cfg(test)]
        {
            let _ = hartid;
        }
    }
}

/// # Safety
///
/// Caller contract: run once per hart after scheduler/smp init, from that
/// hart's boot context; never returns.
pub unsafe fn sched_enter_idle() -> ! {
    // SAFETY: per-hart idle setup: CSR writes configure this hart's trap
    // vector/scratch and interrupt sources; no locks are held in this body
    // and SIE is set only around wfi (audited invariant above).
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

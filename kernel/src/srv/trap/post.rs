//! Post-dispatch bookkeeping: signal delivery, kernel-stack canary check,
//! and the opportunistic reschedule that runs at the end of every trap.
use crate::arch::trap_frame::TrapFrame;
use crate::proc;

/// Run signal delivery, the kernel-stack overflow check and the
/// opportunistic reschedule, after the interrupt/exception itself has been
/// serviced by `super::handle`.
///
/// # Safety
///
/// Must be called from the top-level trap dispatcher (`super::handle`) with
/// `tf` the live, exclusively-owned trap frame for this trap, in S-mode
/// with SIE cleared; relies on the same contract as `super::handle`.
/// `interrupted_user` must reflect whether THIS trap interrupted user mode
/// (SPP=0), captured before any nested handling — see the top-level-vs-
/// nested-frame guard below. May never return for exited processes
/// (context-switches away or halts).
pub unsafe fn run(tf: &mut TrapFrame, scause: u64, interrupted_user: bool) {
    // SAFETY: called only from `super::handle`, which upholds the
    // S-mode / SIE-cleared / live-trap-context contract documented above.
    unsafe {
        // Signal delivery: check the current process for pending unblocked
        // signals. KILL terminates the process; other signals are cleared (MVP).
        proc::signal_check(tf);
        // Kernel-stack overflow detector: alloc_proc plants KSTACK_CANARY at
        // the bottom of the embedded kstack, where nothing legitimate ever
        // writes (stack usage grows down from the top of the array). A clobbered
        // canary means some syscall path exceeded KSTACK_SIZE and has been
        // smashing the Proc header — pid/ring/state read as garbage afterwards,
        // which historically turned a live login into "pid=0" and made SYS_exit
        // a silent no-op. Log it loudly so the offending path gets identified
        // instead of failing mysteriously later.
        {
            let cur = proc::current_opt();
            if let Some(p) = cur {
                let canary = core::ptr::read_volatile(p.kstack.as_ptr() as *const u64);
                static OVFL_LOGGED: core::sync::atomic::AtomicU32 =
                    core::sync::atomic::AtomicU32::new(0);
                if canary != crate::proc::KSTACK_CANARY
                    && OVFL_LOGGED.load(core::sync::atomic::Ordering::Relaxed) < 8
                {
                    OVFL_LOGGED.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
                    crate::kerr!(
                        "trap",
                        "KERNEL STACK OVERFLOW: kstack canary corrupted pid=%d ring=%d scause=%p sepc=%p",
                        onyx_core::fmt::Arg::from(p.pid),
                        onyx_core::fmt::Arg::from(p.ring as u32),
                        onyx_core::fmt::Arg::from(scause),
                        onyx_core::fmt::Arg::from(tf.sepc)
                    );
                }
            }
        }
        let pid = proc::current_pid();
        if pid != 0
            && let Some(p) = proc::by_pid(pid)
            && matches!(p.state, proc::ProcState::Exited)
        {
            proc::sched_yield(tf);
            // sched_yield returns only if it couldn't context-switch away.
            // For secondary harts, sched_yield switches to idle (never returns).
            // If we reach here, no runnable process exists — halt.
            crate::srv::klog::halt();
        }
        // SMP (wave 2): this must run for idle harts too (pid == 0). A timer
        // tick on a hart sleeping in wfi is what gives it a scheduling
        // opportunity: sched_tick sets G_NEED_RESCHED unconditionally, and
        // sched_yield then dequeues local work or steals from another hart.
        // Per-hart arming is verified in srv/timer.rs: M-mode writes
        // CLINT+mtimecmp+hartid*8 (regs::clint_mtimecmp_hart), S-mode uses the
        // SBI set_timer call, which is inherently per-hart. All of this runs
        // with SIE cleared (hardware on trap entry, restored cleared by
        // trap_return), so every spinlock taken here is interrupt-safe.
        //
        // Guard (todo.md, wfi-deep-in-syscall page fault, 2026-09-03):
        // sched_yield only knows how to save/resume a TOP-LEVEL trap frame
        // — either the idle hart's own wfi loop (current is null) or the
        // frame at the very top of a process's kernel stack, the one
        // `trap_entry` builds when a process traps in from user mode
        // (interrupted_user, SPP=0). A syscall that re-enables SIE and
        // calls `wfi()` several calls deep (e.g. a blocking netstack wait)
        // interrupts KERNEL-mode code instead (SPP=1, current non-null):
        // `tf` there is an inner, nested frame, not that top-level one.
        // Rescheduling off of it here corrupts the process's saved state
        // and crashes on its next syscall. Since the interrupt itself is
        // always serviced above regardless (ticks/watchdog/event pump all
        // ran), skipping the reschedule here just defers it to the next
        // safe point — the next top-level trap on this hart — instead of
        // dropping it: G_NEED_RESCHED stays set and is picked up then.
        let safe_to_reschedule = interrupted_user || proc::current_opt().is_none();
        if safe_to_reschedule
            && proc::process::G_NEED_RESCHED[proc::process::hart_id()]
                .load(core::sync::atomic::Ordering::Acquire)
        {
            proc::sched_yield(tf);
        }
    }
}

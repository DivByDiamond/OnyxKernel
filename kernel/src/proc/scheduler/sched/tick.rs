use core::sync::atomic::Ordering;

use crate::proc::process::{G_NEED_RESCHED, MAX_HARTS, hart_id};

/// # Safety
///
/// Caller contract: trap context on this hart (hart_id() valid); merely
/// sets this hart's G_NEED_RESCHED flag (release store).
pub unsafe fn sched_tick() {
    let hartid = hart_id();
    // SMP (wave 2): idle harts (current == null) must also request
    // scheduling. The tick trap is what wakes an idle hart out of wfi;
    // the resulting sched_yield dequeues local work or steals from a
    // remote runqueue and switches into it. Without this, secondary
    // harts booted into idle never picked up any work.
    G_NEED_RESCHED[hartid].store(true, Ordering::Release);
}

/// # Safety
///
/// Caller contract: bounds-checked store of another hart's resched flag;
/// the flag is per-hart atomics, so cross-hart stores are data-race-free.
pub unsafe fn set_need_resched(hartid: usize, v: bool) {
    if hartid < MAX_HARTS {
        G_NEED_RESCHED[hartid].store(v, Ordering::Release);
    }
}

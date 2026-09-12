use core::{ptr, sync::atomic::Ordering};

use super::super::runqueue::{G_RQ, dequeue, enqueue, rq_lock, rq_unlock};
use super::steal::steal;
use crate::{
    arch::{csr, regs::SSTATUS_SIE, trap_frame::TrapFrame},
    proc::process::{
        G_HART_IDLE_TF, G_HART_IDLE_TF_VALID, G_NEED_RESCHED, KSTACK_SIZE, MAX_HARTS, ProcState,
        current_for_hart, hart_id, set_current_for_hart,
    },
};

/// # Safety
///
/// Caller contract: trap-return/yield context on this hart; this hart's
/// runqueue may be locked only via the rq_lock calls below (no nested
/// rq_lock for the same hart by the caller); tf is the current trap frame.
pub unsafe fn sched_yield(tf: &mut TrapFrame) {
    // SAFETY: all G_RQ / G_HART_* accesses follow the per-hart rq_lock
    // discipline (local queue under rq_lock, remote queues under try_lock);
    // procs dequeued are live heap nodes owned by this hart for the switch.
    unsafe {
        let hartid = hart_id();
        let current = current_for_hart(hartid);

        rq_lock(hartid);

        if current.is_null() {
            G_HART_IDLE_TF[hartid] = *tf;
            G_HART_IDLE_TF_VALID[hartid].store(true, Ordering::Release);
        } else {
            (*current).tf = *tf;
            if matches!((*current).state, ProcState::Running) {
                (*current).state = ProcState::Ready;
                enqueue(hartid, current);
            }
        }

        let mut next = dequeue(hartid);

        // Root-cause fix (SMP respawn crash, 2026-09-08): a dequeued node may
        // be a process that is concurrently inside exit() on ANOTHER hart.
        // exit() removes the process from every runqueue while holding each
        // queue's rq_lock in turn (NOT atomically across harts), so a
        // dequeue/steal on this hart can win the race for a queue that exit()
        // has not locked yet. The node we then hold is published Exited, its
        // fds/AS are torn down, and the reaper kfree()s it — the very next
        // spawn's alloc_proc kmallocs the same memory. Resuming such an
        // in-flight pointer splats the DEAD child's saved user trap frame
        // (ra/regs = login text addresses) over the fresh Proc and
        // context-switches into it: live signature observed under -smp 2 was
        // "kernel-mode pid=1 page fault at ra=sepc=0x12000" — an address
        // inside the ALREADY EXITED login.onx text (seg 0x10000..0x13650),
        // fetched with the kernel satp (scause 0xC), plus a twin variant
        // faulting on a corrupted G_HART_CURRENT slot (load fault at 0x1,
        // scause 0x5). Guard: while still holding OUR rq_lock, re-check the
        // dequeued node's state; anything not enqueuable-by-us (Exited =
        // mid-exit, Free = freed/reused, Creating = not yet published,
        // Stopped = parked, Waiting = blocked) must not be resumed. Put it
        // back on OUR queue only if it is still Ready (exit() has not yet
        // reached its remove loop for this queue — its own remove() will
        // unlink it, and enqueue's on_rq check keeps the flags consistent);
        // otherwise drop it and let the next dequeue/steal find real work.
        // The stale candidate is simply abandoned here — never dereferenced
        // again after the state read, and never resumed.
        if !next.is_null() {
            let st = (*next).state;
            if !matches!(st, ProcState::Ready) {
                next = core::ptr::null_mut();
            }
        }

        // Bug (proc MINOR #6): if dequeue returns the same process we just
        // enqueued (the only runnable process on this hart), don't bother
        // doing a context switch to ourselves — that's wasted work (save
        // trap frame, switch stack, restore the same trap frame). Just
        // promote it back to Running and return. This is a very common
        // case when a single process is running on a hart.
        if next == current && !next.is_null() {
            (*next).state = ProcState::Running;
            rq_unlock(hartid);
            G_NEED_RESCHED[hartid].store(false, Ordering::Release);
            // Root-cause fix (blank-console / dead-100Hz-timer investigation,
            // 2026-09-12): this is the busy-poll self-yield path — the only
            // runnable process on this hart yielded back to itself (e.g.
            // console_read spinning on read(0) with nothing else to run).
            // Returning straight back into the caller's kernel loop left
            // sstatus.SIE at 0 (inherited from trap entry) forever: a
            // pending CLINT tick (sip.STIP) could never be delivered, so
            // srv::timer::handle() never ran, its SBI_SET_TIMER ecall never
            // re-armed mie.MTIE (arch/asm/mtrap.rs clears MTIE when
            // forwarding a tick and only re-arms it from that ecall), and
            // the 100 Hz tick died forever after the interrupt already in
            // flight at the moment this path was first hit — taking
            // preemption, the watchdog, soft timers and the console
            // presenter (fb_term::ansi::present_tick) down with it.
            //
            // No SpinLock is held here (rq_unlock already ran above), so
            // this mirrors the ONE other audited place that runs with
            // SIE set (proc::scheduler::idle::sched_enter_idle, see the
            // interrupt invariant in crate::sync): open a short interrupt
            // window so a pending tick is taken and serviced, then restore
            // the kernel's SIE=0 invariant before returning to the caller.
            // No additional `unsafe` needed: this whole function body already
            // runs inside the caller's unsafe block (see the fn signature).
            csr::set_sstatus(SSTATUS_SIE);
            core::arch::asm!("nop");
            csr::clear_sstatus(SSTATUS_SIE);
            return;
        }

        if !next.is_null() {
            let affinity = (*next).affinity;
            let target = if affinity >= 0 && (affinity as usize) < MAX_HARTS {
                affinity as usize
            } else {
                hartid
            };
            if target != hartid {
                // Audit fix: enqueueing into a REMOTE runqueue without holding
                // that queue's rq_lock was the same bug class as Bug #11 (fixed
                // in steal()). Mirror steal()'s pattern here: try-lock the remote
                // queue and only migrate while it is held. If the lock is busy we
                // put the process back on our own queue — a later tick/steal will
                // retry the migration — instead of mutating the remote queue
                // unlocked.
                if (*G_RQ.as_mut_ptr())[target].lock.try_lock() {
                    enqueue(target, next);
                    (*G_RQ.as_mut_ptr())[target].lock.unlock();
                    next = dequeue(hartid);
                } else {
                    enqueue(hartid, next);
                    next = dequeue(hartid);
                }
            }
        }

        if next.is_null() {
            rq_unlock(hartid);
            let stolen = steal(hartid);
            // Same stale-candidate guard as the local dequeue above (see the
            // root-cause fix comment there): steal() dequeues under the
            // VICTIM's lock, but exit()'s per-queue removal loop may simply
            // not have reached the victim's queue yet, so the stolen node can
            // already be Exited (or worse: freed/reused). A non-Ready node is
            // abandoned without being resumed; if it is still Ready we own a
            // legitimate steal.
            if !stolen.is_null() {
                let st = (*stolen).state;
                if matches!(st, ProcState::Ready) {
                    (*stolen).state = ProcState::Running;
                    set_current_for_hart(hartid, stolen);
                    G_NEED_RESCHED[hartid].store(false, Ordering::Release);
                    let kstack_top = (*stolen).kstack.as_ptr().add(KSTACK_SIZE) as usize;
                    let dst = (kstack_top - core::mem::size_of::<TrapFrame>()) as *mut TrapFrame;
                    ptr::write_volatile(dst, (*stolen).tf);
                    crate::arch::asm::sched_switch(dst as usize);
                }
            }
            rq_lock(hartid);
            next = dequeue(hartid);
        }

        if next.is_null() {
            if current.is_null() {
                rq_unlock(hartid);
                G_NEED_RESCHED[hartid].store(false, Ordering::Release);
                return;
            }
            if matches!((*current).state, ProcState::Exited) {
                // Switch this hart to its idle context instead of halting the
                // machine. Previously, hart 0 would `klog::halt()` here, which
                // took the whole system down on the first process exit. Now all
                // harts behave uniformly: drop the exited process as current and
                // resume the idle trap frame saved on entry to sched_yield.
                set_current_for_hart(hartid, ptr::null_mut());
                rq_unlock(hartid);
                G_NEED_RESCHED[hartid].store(false, Ordering::Release);
                let stack_top = crate::arch::smp::G_SEC_STACKS.as_ptr() as usize
                    + (hartid + 1) * crate::arch::smp::SEC_STACK_SIZE;
                // Defensive re-seed: root cause not yet found (todo.md, open
                // item) for a rarer, separate fault where a hart's idle
                // context is found with sepc==0 right before this resume —
                // address 0 is never a legitimate resume target — or was
                // simply never captured at all (only possible for the boot
                // hart before `seed_boot_hart_idle_context` ran, see
                // `srv::main::init::launch`). Either way, re-seed a fresh
                // valid context and fall through to the SAME ordinary
                // `sched_switch`/`trap_return` resume used for every other
                // process switch below, rather than a hand-rolled raw-asm
                // jump — an earlier attempt at that (raw `mv sp`/`jr`) was
                // itself fragile and is why this reuses the well-exercised
                // path instead. This masks the symptom, not the cause; see
                // todo.md for what's still open.
                if !G_HART_IDLE_TF_VALID[hartid].load(Ordering::Acquire)
                    || G_HART_IDLE_TF[hartid].sepc == 0
                {
                    super::super::seed_boot_hart_idle_context(hartid);
                }
                let dst = (stack_top - core::mem::size_of::<TrapFrame>()) as *mut TrapFrame;
                ptr::write_volatile(dst, G_HART_IDLE_TF[hartid]);
                crate::arch::asm::sched_switch(dst as usize);
            }
            // Only flip a preempted Running/Ready process back to Running. A
            // process that is Waiting (on a child, pipe, etc.) or otherwise
            // blocked must NOT be scheduled here — restoring Running would
            // defeat wait()/waitpid() and run the process prematurely.
            if matches!((*current).state, ProcState::Ready) {
                (*current).state = ProcState::Running;
            }
            rq_unlock(hartid);
            G_NEED_RESCHED[hartid].store(false, Ordering::Release);
            return;
        }

        (*next).state = ProcState::Running;
        set_current_for_hart(hartid, next);
        rq_unlock(hartid);
        G_NEED_RESCHED[hartid].store(false, Ordering::Release);

        let next_kstack_top = (*next).kstack.as_ptr().add(KSTACK_SIZE) as usize;
        let dst = (next_kstack_top - core::mem::size_of::<TrapFrame>()) as *mut TrapFrame;
        ptr::write_volatile(dst, (*next).tf);
        crate::arch::asm::sched_switch(dst as usize);
    }
}

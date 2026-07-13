use super::runqueue::{dequeue, enqueue, rq_lock, rq_unlock, G_RQ};
use crate::arch::trap_frame::TrapFrame;
use crate::proc::process::{
    current_for_hart, hart_id, set_current_for_hart, Proc, ProcState, G_HART_IDLE_TF,
    G_NEED_RESCHED, KSTACK_SIZE, MAX_HARTS,
};
use core::ptr;
use core::sync::atomic::Ordering;

pub unsafe fn sched_tick() {
    let hartid = hart_id();
    let cur = current_for_hart(hartid);
    if !cur.is_null() && !matches!((*cur).state, ProcState::Free) {
        G_NEED_RESCHED[hartid].store(true, Ordering::Release);
    }
}

pub unsafe fn set_need_resched(hartid: usize, v: bool) {
    if hartid < MAX_HARTS {
        G_NEED_RESCHED[hartid].store(v, Ordering::Release);
    }
}

pub unsafe fn steal(hartid: usize) -> *mut Proc {
    let n = MAX_HARTS;
    for i in 1..n {
        let victim = (hartid + i) % n;
        if victim == hartid {
            continue;
        }
        // Bug #11 fix: hold the victim's rq_lock across dequeue AND any
        // re-enqueue caused by an affinity mismatch. Previously the lock
        // was released immediately after dequeue and the subsequent
        // `enqueue(victim, p)` for an affinity-mismatched process mutated
        // the victim's runqueue without any lock, racing with the victim
        // hart's own scheduler and producing orphaned/duplicated entries.
        if (*G_RQ.as_mut_ptr())[victim]
            .lock
            .swap(true, Ordering::Acquire)
        {
            continue;
        }
        let p = dequeue(victim);
        if !p.is_null() {
            let affinity = (*p).affinity;
            if affinity >= 0 && (affinity as usize) != hartid {
                // Put it back on the victim's queue (lock still held).
                enqueue(victim, p);
                (*G_RQ.as_mut_ptr())[victim]
                    .lock
                    .store(false, Ordering::Release);
                continue;
            }
            // Got a stealable process — release the lock and return it.
            (*G_RQ.as_mut_ptr())[victim]
                .lock
                .store(false, Ordering::Release);
            return p;
        }
        // Nothing to steal from this victim — release the lock.
        (*G_RQ.as_mut_ptr())[victim]
            .lock
            .store(false, Ordering::Release);
    }
    core::ptr::null_mut()
}

pub unsafe fn sched_yield(tf: &mut TrapFrame) {
    let hartid = hart_id();
    let current = current_for_hart(hartid);

    rq_lock(hartid);

    if current.is_null() {
        G_HART_IDLE_TF[hartid] = *tf;
    } else {
        (*current).tf = *tf;
        if matches!((*current).state, ProcState::Running) {
            (*current).state = ProcState::Ready;
            enqueue(hartid, current);
        }
    }

    let mut next = dequeue(hartid);

    if !next.is_null() {
        let affinity = (*next).affinity;
        if affinity >= 0 && (affinity as usize) != hartid {
            enqueue(affinity as usize, next);
            next = dequeue(hartid);
        }
    }

    if next.is_null() {
        rq_unlock(hartid);
        let stolen = steal(hartid);
        if !stolen.is_null() {
            (*stolen).state = ProcState::Running;
            set_current_for_hart(hartid, stolen);
            G_NEED_RESCHED[hartid].store(false, Ordering::Release);
            let kstack_top = (*stolen).kstack.as_ptr().add(KSTACK_SIZE) as usize;
            let dst = (kstack_top - core::mem::size_of::<TrapFrame>()) as *mut TrapFrame;
            ptr::write_volatile(dst, (*stolen).tf);
            crate::arch::asm::sched_switch(dst as usize);
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

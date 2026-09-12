use super::super::runqueue::{G_RQ, dequeue, enqueue};
use crate::proc::process::{MAX_HARTS, Proc};

/// # Safety
///
/// Caller contract: init() has run (G_RQ initialized); kernel context with
/// SIE clear. The victim queue lock is held (try_lock) across dequeue and
/// any re-enqueue; the returned Proc (if any) is owned by the caller.
pub unsafe fn steal(hartid: usize) -> *mut Proc {
    // SAFETY: G_RQ was initialized by runqueue::init(); each victim queue
    // is only dereferenced/mutated while its try_lock() is held below, and
    // dequeued procs are valid live heap nodes under the rq lock discipline.
    unsafe {
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
            if !(*G_RQ.as_mut_ptr())[victim].lock.try_lock() {
                continue;
            }
            let p = dequeue(victim);
            if !p.is_null() {
                let affinity = (*p).affinity;
                if affinity >= 0 && (affinity as usize) != hartid {
                    // Put it back on the victim's queue (lock still held).
                    enqueue(victim, p);
                    (*G_RQ.as_mut_ptr())[victim].lock.unlock();
                    continue;
                }
                // Got a stealable process — release the lock and return it.
                (*G_RQ.as_mut_ptr())[victim].lock.unlock();
                return p;
            }
            // Nothing to steal from this victim — release the lock.
            (*G_RQ.as_mut_ptr())[victim].lock.unlock();
        }
        core::ptr::null_mut()
    }
}

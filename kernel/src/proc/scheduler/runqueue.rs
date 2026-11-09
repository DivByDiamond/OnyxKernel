use core::mem::MaybeUninit;

use crate::{
    proc::process::{MAX_HARTS, Proc},
    sync::SpinLock,
};

pub struct RunQueue {
    pub lock: SpinLock,
    pub head: *mut Proc,
    pub tail: *mut Proc,
    pub nr_ready: usize,
}

// SAFETY: RunQueue is only manipulated while holding its own SpinLock
// (or from the owning hart before other harts can observe it).
unsafe impl Sync for RunQueue {}

pub static mut G_RQ: MaybeUninit<[RunQueue; MAX_HARTS]> = MaybeUninit::uninit();

pub fn init() {
    // SAFETY: boot-time init of G_RQ before any hart schedules.
    unsafe {
        for i in 0..MAX_HARTS {
            (*G_RQ.as_mut_ptr())[i] = RunQueue {
                lock: SpinLock::new(),
                head: core::ptr::null_mut(),
                tail: core::ptr::null_mut(),
                nr_ready: 0,
            };
        }
    }
}

/// # Safety
///
/// Caller contract: init() has run; `hart` < MAX_HARTS.
unsafe fn rq(hart: usize) -> &'static mut RunQueue {
    // SAFETY: G_RQ was initialized by init() and never re-assigned; valid index per contract.
    unsafe { &mut (*G_RQ.as_mut_ptr())[hart] }
}

/// # Safety
///
/// Caller contract: init() has run; `hart` < MAX_HARTS; SIE clear (crate::sync invariant).
pub unsafe fn rq_lock(hart: usize) {
    // SAFETY: rq(hart) valid per caller contract; SpinLock interrupt invariant holds.
    unsafe {
        rq(hart).lock.lock();
    }
}

/// # Safety
///
/// Caller contract: `hart` < MAX_HARTS and rq_lock(hart) is held by this hart.
pub unsafe fn rq_unlock(hart: usize) {
    // SAFETY: rq(hart) valid; unlock pairs with the caller's prior rq_lock(hart).
    unsafe {
        rq(hart).lock.unlock();
    }
}

/// # Safety
///
/// Caller contract: rq_lock(hart) held; `p` a live heap Proc not on another runqueue.
pub unsafe fn enqueue(hart: usize, p: *mut Proc) {
    // SAFETY: rq(hart) and p dereferenceable; rq_lock(hart) held excludes queue races.
    unsafe {
        if (*p).on_rq {
            return;
        }
        (*p).on_rq = true;
        (*p).next = core::ptr::null_mut();
        if rq(hart).tail.is_null() {
            rq(hart).head = p;
            rq(hart).tail = p;
        } else {
            (*(rq(hart).tail)).next = p;
            rq(hart).tail = p;
        }
        rq(hart).nr_ready += 1;
    }
}

/// # Safety
///
/// Caller contract: as enqueue - the target queue's rq_lock must be held; `p` live.
pub unsafe fn enqueue_affine(hart: usize, p: *mut Proc) {
    // SAFETY: p dereferenceable per caller contract; enqueue runs under the target's lock.
    unsafe {
        let target = if (*p).affinity >= 0 && (*p).affinity < MAX_HARTS as i32 {
            (*p).affinity as usize
        } else {
            hart
        };
        enqueue(target, p);
    }
}

/// # Safety
///
/// Caller contract: rq_lock(hart) held; `hart` < MAX_HARTS; returned Proc owned by caller.
pub unsafe fn dequeue(hart: usize) -> *mut Proc {
    // SAFETY: rq(hart) valid; rq_lock(hart) held excludes concurrent queue mutation.
    unsafe {
        let p = rq(hart).head;
        if !p.is_null() {
            rq(hart).head = (*p).next;
            if rq(hart).head.is_null() {
                rq(hart).tail = core::ptr::null_mut();
            }
            (*p).next = core::ptr::null_mut();
            (*p).on_rq = false;
            rq(hart).nr_ready -= 1;
        }
        p
    }
}

/// # Safety
///
/// Caller contract: rq_lock(hart) held; `p` a valid Proc on this queue.
pub unsafe fn remove(hart: usize, p: *mut Proc) -> bool {
    // SAFETY: rq(hart) and p dereferenceable; rq_lock(hart) held excludes queue races.
    unsafe {
        if !(*p).on_rq {
            return false;
        }
        let mut prev: *mut Proc = core::ptr::null_mut();
        let mut cur = rq(hart).head;
        while !cur.is_null() {
            if cur == p {
                if prev.is_null() {
                    rq(hart).head = (*cur).next;
                } else {
                    (*prev).next = (*cur).next;
                }
                if rq(hart).tail == p {
                    rq(hart).tail = prev;
                }
                (*p).next = core::ptr::null_mut();
                (*p).on_rq = false;
                rq(hart).nr_ready -= 1;
                return true;
            }
            prev = cur;
            cur = (*cur).next;
        }
        false
    }
}

#[cfg(test)]
mod tests;

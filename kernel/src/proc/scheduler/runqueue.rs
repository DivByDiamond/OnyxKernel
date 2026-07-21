use crate::proc::process::{Proc, MAX_HARTS};
use core::hint::spin_loop;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicBool, Ordering};

pub struct RunQueue {
    pub lock: AtomicBool,
    pub head: *mut Proc,
    pub tail: *mut Proc,
    pub nr_ready: usize,
}

unsafe impl Sync for RunQueue {}

pub static mut G_RQ: MaybeUninit<[RunQueue; MAX_HARTS]> = MaybeUninit::uninit();

pub fn init() {
    unsafe {
        for i in 0..MAX_HARTS {
            (*G_RQ.as_mut_ptr())[i] = RunQueue {
                lock: AtomicBool::new(false),
                head: core::ptr::null_mut(),
                tail: core::ptr::null_mut(),
                nr_ready: 0,
            };
        }
    }
}

unsafe fn rq(hart: usize) -> &'static mut RunQueue {
    &mut (*G_RQ.as_mut_ptr())[hart]
}

pub unsafe fn rq_lock(hart: usize) {
    while rq(hart).lock.swap(true, Ordering::Acquire) {
        while rq(hart).lock.load(Ordering::Relaxed) {
            spin_loop();
        }
    }
}

pub unsafe fn rq_unlock(hart: usize) {
    rq(hart).lock.store(false, Ordering::Release);
}

pub unsafe fn enqueue(hart: usize, p: *mut Proc) {
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

pub unsafe fn enqueue_affine(hart: usize, p: *mut Proc) {
    let target = if (*p).affinity >= 0 && (*p).affinity < MAX_HARTS as i32 {
        (*p).affinity as usize
    } else {
        hart
    };
    enqueue(target, p);
}

pub unsafe fn dequeue(hart: usize) -> *mut Proc {
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

pub unsafe fn remove(hart: usize, p: *mut Proc) -> bool {
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

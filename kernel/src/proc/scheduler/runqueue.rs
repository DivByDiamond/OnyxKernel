use crate::proc::process::{MAX_HARTS, Proc};
use core::hint::spin_loop;
use core::sync::atomic::{AtomicBool, Ordering};

pub struct RunQueue {
    pub lock: AtomicBool,
    pub head: *mut Proc,
    pub tail: *mut Proc,
    pub nr_ready: usize,
}

unsafe impl Sync for RunQueue {}

pub static mut G_RQ: [RunQueue; MAX_HARTS] =
    const { unsafe { core::mem::transmute([0u8; core::mem::size_of::<RunQueue>() * MAX_HARTS]) } };

pub unsafe fn rq_lock(hart: usize) {
    while G_RQ[hart].lock.swap(true, Ordering::Acquire) {
        while G_RQ[hart].lock.load(Ordering::Relaxed) {
            spin_loop();
        }
    }
}

pub unsafe fn rq_unlock(hart: usize) {
    G_RQ[hart].lock.store(false, Ordering::Release);
}

pub unsafe fn enqueue(hart: usize, p: *mut Proc) {
    if (*p).on_rq {
        return;
    }
    (*p).on_rq = true;
    (*p).next = core::ptr::null_mut();
    if G_RQ[hart].tail.is_null() {
        G_RQ[hart].head = p;
        G_RQ[hart].tail = p;
    } else {
        (*(G_RQ[hart].tail)).next = p;
        G_RQ[hart].tail = p;
    }
    G_RQ[hart].nr_ready += 1;
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
    let p = G_RQ[hart].head;
    if !p.is_null() {
        G_RQ[hart].head = (*p).next;
        if G_RQ[hart].head.is_null() {
            G_RQ[hart].tail = core::ptr::null_mut();
        }
        (*p).next = core::ptr::null_mut();
        (*p).on_rq = false;
        G_RQ[hart].nr_ready -= 1;
    }
    p
}

pub unsafe fn remove(hart: usize, p: *mut Proc) -> bool {
    if !(*p).on_rq {
        return false;
    }
    let mut prev: *mut Proc = core::ptr::null_mut();
    let mut cur = G_RQ[hart].head;
    while !cur.is_null() {
        if cur == p {
            if prev.is_null() {
                G_RQ[hart].head = (*cur).next;
            } else {
                (*prev).next = (*cur).next;
            }
            if G_RQ[hart].tail == p {
                G_RQ[hart].tail = prev;
            }
            (*p).next = core::ptr::null_mut();
            (*p).on_rq = false;
            G_RQ[hart].nr_ready -= 1;
            return true;
        }
        prev = cur;
        cur = (*cur).next;
    }
    false
}

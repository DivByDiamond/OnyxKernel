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

unsafe impl Sync for RunQueue {}

pub static mut G_RQ: MaybeUninit<[RunQueue; MAX_HARTS]> = MaybeUninit::uninit();

pub fn init() {
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

unsafe fn rq(hart: usize) -> &'static mut RunQueue {
    unsafe { &mut (*G_RQ.as_mut_ptr())[hart] }
}

pub unsafe fn rq_lock(hart: usize) {
    unsafe {
        rq(hart).lock.lock();
    }
}

pub unsafe fn rq_unlock(hart: usize) {
    unsafe {
        rq(hart).lock.unlock();
    }
}

pub unsafe fn enqueue(hart: usize, p: *mut Proc) {
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

pub unsafe fn enqueue_affine(hart: usize, p: *mut Proc) {
    unsafe {
        let target = if (*p).affinity >= 0 && (*p).affinity < MAX_HARTS as i32 {
            (*p).affinity as usize
        } else {
            hart
        };
        enqueue(target, p);
    }
}

pub unsafe fn dequeue(hart: usize) -> *mut Proc {
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

pub unsafe fn remove(hart: usize, p: *mut Proc) -> bool {
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
mod tests {
    use super::super::sched::steal;
    use super::*;

    fn proc_node(pid: u32, affinity: i32) -> *mut Proc {
        let mut p = Box::new(Proc::new());
        p.pid = pid;
        p.affinity = affinity;
        Box::into_raw(p)
    }

    unsafe fn rq_len(hart: usize) -> usize {
        unsafe { (*G_RQ.as_mut_ptr())[hart].nr_ready }
    }

    /// Combined test: G_RQ is a process-global static and the host test
    /// harness runs #[test] fns in parallel, so every runqueue/steal
    /// assertion lives in one function to stay deterministic.
    #[test]
    fn test_runqueue_fifo_affinity_remove_and_steal() {
        unsafe {
            init();

            // FIFO order: a, b, c on hart 0 come back out in enqueue order.
            let a = proc_node(11, -1);
            let b = proc_node(12, -1);
            let c = proc_node(13, -1);
            enqueue(0, a);
            enqueue(0, b);
            enqueue(0, c);
            assert_eq!(rq_len(0), 3);
            // Double enqueue of an already-queued process is a no-op.
            enqueue(0, a);
            assert_eq!(rq_len(0), 3);
            assert_eq!((*dequeue(0)).pid, 11);
            assert_eq!((*dequeue(0)).pid, 12);
            assert_eq!((*dequeue(0)).pid, 13);
            // Dequeue from an empty queue yields null.
            assert!(dequeue(0).is_null());
            assert_eq!(rq_len(0), 0);

            // remove(): the tail leaves the queue; a second remove fails.
            let d = proc_node(14, -1);
            let e = proc_node(15, -1);
            enqueue(0, d);
            enqueue(0, e);
            assert!(remove(0, e));
            assert_eq!(rq_len(0), 1);
            assert!(!remove(0, e));
            assert_eq!((*dequeue(0)).pid, 14);
            assert!(dequeue(0).is_null());

            // Affinity: enqueue_affine routes to the pinned hart, and an
            // out-of-range pin falls back to the caller's hart.
            let pinned = proc_node(16, 3);
            enqueue_affine(0, pinned);
            assert_eq!(rq_len(0), 0);
            assert_eq!(rq_len(3), 1);
            assert_eq!((*dequeue(3)).pid, 16);
            let wild = proc_node(20, 99);
            enqueue_affine(2, wild);
            assert_eq!(rq_len(2), 1);
            assert_eq!((*dequeue(2)).pid, 20);

            // Steal 1: an unpinned process on a remote hart is stealable.
            let stealable = proc_node(17, -1);
            enqueue(1, stealable);
            let got = steal(0);
            assert!(!got.is_null());
            assert_eq!((*got).pid, 17);

            // Steal 2: a process pinned to the victim hart is put back and
            // NOT stolen.
            let pinned1 = proc_node(18, 1);
            enqueue(1, pinned1);
            assert!(steal(0).is_null());
            assert_eq!(rq_len(1), 1);
            assert!((*pinned1).on_rq);
            assert_eq!((*dequeue(1)).pid, 18);

            // Steal 3: a process pinned to the stealing hart is stealable.
            let pinned0 = proc_node(19, 0);
            enqueue(1, pinned0);
            let got = steal(0);
            assert!(!got.is_null());
            assert_eq!((*got).pid, 19);
        }
    }
}

use super::super::sched::steal;
use super::*;

fn proc_node(pid: u32, affinity: i32) -> *mut Proc {
    let mut p = Box::new(Proc::new());
    p.pid = pid;
    p.affinity = affinity;
    Box::into_raw(p)
}

/// # Safety
///
/// Test-only: reads nr_ready; single merged test, no parallel G_RQ access.
unsafe fn rq_len(hart: usize) -> usize {
    // SAFETY: G_RQ initialized by init() in this single merged test; no concurrent access.
    unsafe { (*G_RQ.as_mut_ptr())[hart].nr_ready }
}

/// Combined test: G_RQ is a process-global static and the host test
/// harness runs #[test] fns in parallel, so every runqueue/steal
/// assertion lives in one function to stay deterministic. It shares
/// `proc::tests::test_serial` with the other proc-global tests, which
/// also call `init()` on the same statics.
#[test]
fn test_runqueue_fifo_affinity_remove_and_steal() {
    let _serial = crate::proc::tests::test_serial();
    // SAFETY: host test; G_RQ is initialized here and exercised
    // single-threaded from this one merged test (no parallel access).
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

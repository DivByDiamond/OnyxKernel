use super::*;
use crate::arch::trap_frame::TrapFrame;

#[test]
fn test_alloc_pid_unique() {
    // SAFETY: host test; resets and exercises the process globals directly.
    // Caller contract: single-threaded access - but the host harness runs
    // #[test] fns in parallel, and both alloc_pid tests touch G_NEXT_PID
    // and G_ALL_PROCS (pre-existing flakiness hazard, see report note).
    unsafe {
        super::process::init();
        let pid1 = alloc_pid();
        let pid2 = alloc_pid();
        assert_eq!(pid1, PROC_PID_INIT + 1);
        assert_eq!(pid2, PROC_PID_INIT + 2);
        assert_ne!(pid1, pid2);
    }
}

#[test]
fn test_alloc_pid_increment() {
    // SAFETY: same host-test contract as test_alloc_pid_unique (parallel
    // #[test] fns share the process globals (pre-existing flakiness).
    unsafe {
        super::process::init();
        let base = alloc_pid();
        for i in 1..=10 {
            assert_eq!(alloc_pid(), base + i);
        }
    }
}

#[test]
fn test_ring_constants() {
    assert_eq!(PROC_RING_KERNEL, 0);
    assert_eq!(PROC_RING_ROOT, 1);
    assert_eq!(PROC_RING_USER, 2);
}

#[test]
fn test_proc_state_values() {
    assert_eq!(ProcState::Free as u32, 0);
    assert_eq!(ProcState::Ready as u32, 1);
    assert_eq!(ProcState::Running as u32, 2);
    assert_eq!(ProcState::Exited as u32, 3);
    assert_eq!(ProcState::Waiting as u32, 4);
}

#[test]
fn test_proc_state_equality() {
    assert!(ProcState::Free == ProcState::Free);
    assert!(ProcState::Ready != ProcState::Running);
    assert!(ProcState::Exited != ProcState::Free);
}

#[test]
fn test_trap_frame_zero() {
    let tf = TrapFrame::zero();
    assert_eq!(tf.ra, 0);
    assert_eq!(tf.sp, 0);
    assert_eq!(tf.gp, 0);
    assert_eq!(tf.tp, 0);
    assert_eq!(tf.t0, 0);
    assert_eq!(tf.t1, 0);
    assert_eq!(tf.t2, 0);
    assert_eq!(tf.s0, 0);
    assert_eq!(tf.s1, 0);
    assert_eq!(tf.a0, 0);
    assert_eq!(tf.a1, 0);
    assert_eq!(tf.a2, 0);
    assert_eq!(tf.a3, 0);
    assert_eq!(tf.a4, 0);
    assert_eq!(tf.a5, 0);
    assert_eq!(tf.a6, 0);
    assert_eq!(tf.a7, 0);
    assert_eq!(tf.s2, 0);
    assert_eq!(tf.s3, 0);
    assert_eq!(tf.s4, 0);
    assert_eq!(tf.s5, 0);
    assert_eq!(tf.s6, 0);
    assert_eq!(tf.s7, 0);
    assert_eq!(tf.s8, 0);
    assert_eq!(tf.s9, 0);
    assert_eq!(tf.s10, 0);
    assert_eq!(tf.s11, 0);
    assert_eq!(tf.t3, 0);
    assert_eq!(tf.t4, 0);
    assert_eq!(tf.t5, 0);
    assert_eq!(tf.t6, 0);
    assert_eq!(tf.sepc, 0);
    assert_eq!(tf.sstatus, 0);
    assert_eq!(tf.scause, 0);
    assert_eq!(tf.stval, 0);
    assert_eq!(tf.satp, 0);
}

#[test]
fn test_trap_frame_copy() {
    let mut tf = TrapFrame::zero();
    tf.a0 = 42;
    tf.sepc = 0x8000_0000;
    tf.sp = 0x7FFF_FFF0;
    let tf2 = tf;
    assert_eq!(tf2.a0, 42);
    assert_eq!(tf2.sepc, 0x8000_0000);
    assert_eq!(tf2.sp, 0x7FFF_FFF0);
}

#[test]
fn test_trap_frame_clone() {
    let tf = TrapFrame::zero();
    let tf2 = tf;
    assert_eq!(tf.ra, tf2.ra);
    assert_eq!(tf.sp, tf2.sp);
}

#[test]
fn test_proc_constants() {
    assert_eq!(PROC_PID_INIT, 1);
    // 64 KB since the execve kstack-overflow fix (todo.md item 0).
    assert_eq!(KSTACK_SIZE, 64 * 1024);
    assert_eq!(PROC_MAX_FDS, 16);
}

#[test]
fn test_proc_state_ordering() {
    assert!((ProcState::Free as u32) < (ProcState::Ready as u32));
    assert!((ProcState::Ready as u32) < (ProcState::Running as u32));
    assert!((ProcState::Running as u32) < (ProcState::Exited as u32));
    assert!((ProcState::Exited as u32) < (ProcState::Waiting as u32));
}

// ── Fork-race fix tests (todo P1 #1): Creating state + publish_ready ──────

#[test]
fn test_proc_state_creating_value() {
    // The fork-race fix adds Creating = 5; a Creating process must never be
    // confused with a runnable one.
    assert_eq!(ProcState::Creating as u32, 5);
    assert!(ProcState::Creating != ProcState::Ready);
    assert!(ProcState::Creating != ProcState::Free);
}

/// Combined test: G_ALL_PROCS and the runqueues are process-global statics
/// and the host harness runs #[test] fns in parallel, so every
/// create/publish invariant lives in one function (same pattern as
/// runqueue.rs).
#[test]
fn test_fork_creating_not_runnable_until_publish() {
    use crate::proc::process::{G_ALL_PROCS, by_pid_unlocked, proc_list_lock, proc_list_unlock};
    use crate::proc::scheduler::runqueue;

    unsafe {
        super::process::init();
        runqueue::init();

        // Hand-build a child exactly as create_user leaves it: linked into
        // G_ALL_PROCS, state Creating, NOT enqueued on any runqueue.
        let mut child = Box::new(Proc::new());
        child.pid = 7001;
        child.parent_pid = 7000;
        child.state = ProcState::Creating;
        child.affinity = -1;
        child.on_rq = false;
        let child_ptr = Box::into_raw(child);

        proc_list_lock();
        (*child_ptr).all_next = G_ALL_PROCS;
        G_ALL_PROCS = child_ptr;
        proc_list_unlock();

        // Invariant 1 (the race fix): the scheduler must find NOTHING to run
        // while the child is Creating — a work-stealing hart can never pick
        // it up with zeroed fds.
        assert!(crate::proc::scheduler::dequeue(0).is_null());
        assert!(!(*child_ptr).on_rq);

        // Invariant 2: the child IS visible to by_pid (waitpid's has_child
        // scan must see it as a live child while it is being initialized).
        proc_list_lock();
        assert!(by_pid_unlocked(7001).is_some());
        proc_list_unlock();

        // Invariant 3: publish_ready flips Creating → Ready and enqueues —
        // the atomic publication point at the END of the state copy.
        assert!(crate::proc::publish_ready(7001));
        assert!(matches!((*child_ptr).state, ProcState::Ready));
        assert!((*child_ptr).on_rq);

        // Invariant 4: after publication the scheduler can dequeue it.
        let picked = crate::proc::scheduler::dequeue(0);
        assert_eq!(picked, child_ptr);
        assert!(!(*child_ptr).on_rq);

        // Invariant 5: double publish is rejected (Creating-only transition).
        assert!(!crate::proc::publish_ready(7001));
        assert!(matches!((*child_ptr).state, ProcState::Ready));

        // Invariant 6: publish of an unknown pid fails cleanly.
        assert!(!crate::proc::publish_ready(424_242));

        // Unlink + drop (host test cleanup; free_proc would kfree a Box).
        proc_list_lock();
        if G_ALL_PROCS == child_ptr {
            G_ALL_PROCS = (*child_ptr).all_next;
        } else {
            let mut walk = G_ALL_PROCS;
            while !walk.is_null() && (*walk).all_next != child_ptr {
                walk = (*walk).all_next;
            }
            if !walk.is_null() {
                (*walk).all_next = (*child_ptr).all_next;
            }
        }
        proc_list_unlock();
        drop(Box::from_raw(child_ptr));
    }
}

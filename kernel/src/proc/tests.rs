use super::*;
use crate::arch::trap_frame::TrapFrame;

#[test]
fn test_alloc_pid_unique() {
    unsafe {
        super::process::init();
        let pid1 = alloc_pid();
        let pid2 = alloc_pid();
        assert_eq!(pid1, PROC_PID_INIT);
        assert_eq!(pid2, PROC_PID_INIT + 1);
        assert_ne!(pid1, pid2);
    }
}

#[test]
fn test_alloc_pid_increment() {
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
    let tf2 = tf.clone();
    assert_eq!(tf.ra, tf2.ra);
    assert_eq!(tf.sp, tf2.sp);
}

#[test]
fn test_proc_constants() {
    assert_eq!(PROC_PID_INIT, 1);
    assert_eq!(KSTACK_SIZE, 16 * 1024);
    assert_eq!(PROC_MAX_FDS, 16);
}

#[test]
fn test_proc_state_ordering() {
    assert!((ProcState::Free as u32) < (ProcState::Ready as u32));
    assert!((ProcState::Ready as u32) < (ProcState::Running as u32));
    assert!((ProcState::Running as u32) < (ProcState::Exited as u32));
    assert!((ProcState::Exited as u32) < (ProcState::Waiting as u32));
}

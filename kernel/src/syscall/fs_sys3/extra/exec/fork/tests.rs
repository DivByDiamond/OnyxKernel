use super::fork_child_frame;
use crate::arch::trap_frame::TrapFrame;

#[test]
fn child_frame_passes_argv_envp_and_returns_zero() {
    let mut tf = TrapFrame::zero();
    tf.sp = 0x7fff_abcd;
    tf.sepc = 0x1000;
    tf.a1 = 0xdead; // caller-saved garbage at fork time
    tf.a2 = 0xbeef;
    let child = fork_child_frame(&tf, 0x7fff_1000, 0x7fff_2000);
    assert_eq!(child.a0, 0); // fork returns 0 in the child
    assert_eq!(child.sepc, 0x1004); // resumes past the ecall
    assert_eq!(child.a1, 0x7fff_1000); // canonical argv restored
    assert_eq!(child.a2, 0x7fff_2000); // canonical envp restored
    assert_eq!(child.sp, tf.sp); // shared frame is inherited
}

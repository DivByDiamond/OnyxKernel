use super::fork_child_frame;
use crate::arch::trap_frame::TrapFrame;

/// Regression: `osh` page-faulted on every `cmd &`. The kernel overwrote the
/// child's a1/a2 with argv/envp, but the syscall ABI only changes a0, so the
/// compiler keeps live values (here a slice) in a1/a2 across the `ecall`.
#[test]
fn child_frame_preserves_every_register_except_the_return_value() {
    let mut tf = TrapFrame::zero();
    tf.sp = 0x7fff_abcd;
    tf.sepc = 0x1000;
    tf.a0 = 7;
    tf.a1 = 0xdead;
    tf.a2 = 0xbeef;
    tf.s5 = 0x5555;
    let child = fork_child_frame(&tf);
    assert_eq!(child.a0, 0); // fork returns 0 in the child
    assert_eq!(child.sepc, 0x1004); // resumes past the ecall
    assert_eq!(child.a1, 0xdead);
    assert_eq!(child.a2, 0xbeef);
    assert_eq!(child.s5, 0x5555);
    assert_eq!(child.sp, tf.sp); // shared frame is inherited
}

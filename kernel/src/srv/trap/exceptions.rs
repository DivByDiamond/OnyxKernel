//! Exception-cause handling (syscalls, page faults, illegal instructions,
//! breakpoints, and the unhandled-exception fallback).
use crate::arch::regs::*;
use crate::arch::trap_frame::{TrapFrame, reg_truncate, reg_widen};
use crate::proc;
use crate::syscall::abi::SYS_sigreturn;
use crate::syscall::handler;

/// Dispatch an exception-class `scause` code (the `SCAUSE_INT` bit already
/// stripped by the caller) against the trapped frame.
///
/// # Safety
///
/// Must be called from the top-level trap dispatcher (`super::handle`) with
/// `tf` the live, exclusively-owned trap frame for this trap, in S-mode with
/// SIE cleared; relies on the same contract as `super::handle`. Per
/// info.md rule 5, every branch here must end in a terminal action
/// (`proc::exit`, `halt()`, or a guaranteed in-place return) — no implicit
/// fallthrough.
pub unsafe fn handle(tf: &mut TrapFrame, scause: u64, code: u64) {
    // SAFETY: called only from `super::handle`, which upholds the
    // S-mode / SIE-cleared / live-trap-context contract documented above.
    unsafe {
        match code {
            CAUSE_U_ECALL => {
                // SYS_sigreturn fully restores the saved trap frame inside
                // the handler (it writes *tf = saved_tf). If we then go on
                // to overwrite a0 with the handler's return value and
                // advance sepc by 4, we corrupt the restored state — the
                // signal handler's return address and a0 would be lost.
                // Special-case sigreturn: skip the post-handle fixups.
                let is_sigreturn = reg_widen(tf.a7) == SYS_sigreturn;
                // SYS_exit must never return to userspace: when the handler
                // returns, the process is already torn down (address space
                // destroyed, state = Exited), so "advancing" sepc past the
                // ecall would only fabricate a live-looking frame pointing
                // into dead code. Skip the fixups; the post-trap check at
                // the bottom of this function sees state == Exited and
                // context-switches away instead of ever returning here.
                let is_exit = reg_widen(tf.a7) == crate::syscall::abi::SYS_exit;
                let ret = handler::handle(tf);
                if !is_sigreturn && !is_exit {
                    tf.a0 = reg_truncate(ret as u64);
                    tf.sepc = tf.sepc.wrapping_add(4);
                }
            }
            CAUSE_INST_PF | CAUSE_LD_PF | CAUSE_ST_PF | CAUSE_IAMISS | CAUSE_LDAMISS
            | CAUSE_STAMISS => {
                let pid = proc::current_pid();
                let stval = crate::arch::csr::read_stval();
                let sstatus = crate::arch::csr::read_sstatus();
                let from_kernel = sstatus & SSTATUS_SPP != 0;
                if from_kernel || pid == 0 {
                    let cur = proc::current_opt();
                    let (_cur_pid, cur_ring, cur_root) = if let Some(p) = cur {
                        (p.pid, p.ring, p.root_pa)
                    } else {
                        (0u32, 0u8, 0u64)
                    };
                    let hart = crate::proc::process::hart_id();
                    let gc = crate::proc::process::current_for_hart(hart) as usize;
                    crate::kerr!(
                        "trap",
                        "KERNEL page fault pid=%d scause=%p sepc=%p stval=%p satp=%p root_pa=%p ring=%d GC=%p ra=%p a7=%d",
                        onyx_core::fmt::Arg::from(pid),
                        onyx_core::fmt::Arg::from(scause),
                        onyx_core::fmt::Arg::from(tf.sepc),
                        onyx_core::fmt::Arg::from(stval),
                        onyx_core::fmt::Arg::from(crate::arch::csr::read_satp()),
                        onyx_core::fmt::Arg::from(cur_root),
                        onyx_core::fmt::Arg::from(cur_ring as u32),
                        onyx_core::fmt::Arg::from(gc as u64),
                        onyx_core::fmt::Arg::from(tf.ra),
                        onyx_core::fmt::Arg::from(reg_widen(tf.a7))
                    );
                    crate::srv::klog::halt();
                }
                crate::kerr!(
                    "trap",
                    "page fault pid=%d sepc=%p stval=%p",
                    onyx_core::fmt::Arg::from(pid),
                    onyx_core::fmt::Arg::from(tf.sepc),
                    onyx_core::fmt::Arg::from(stval)
                );
                proc::exit(pid, 100 + code as i32);
            }
            CAUSE_ILL => {
                let pid = proc::current_pid();
                crate::kerr!(
                    "trap",
                    "illegal instruction hart=%d pid=%d sepc=%p sp=%p ra=%p",
                    onyx_core::fmt::Arg::from(crate::proc::process::hart_id() as u64),
                    onyx_core::fmt::Arg::from(pid),
                    onyx_core::fmt::Arg::from(tf.sepc),
                    onyx_core::fmt::Arg::from(tf.sp),
                    onyx_core::fmt::Arg::from(tf.ra)
                );
                // Defensive fix: pid==0 means this trap has no process
                // context (idle hart, or kernel-mode code with no
                // `current`) — `proc::exit(0, ..)` is a no-op (by_pid(0)
                // never matches), so without this the trap falls through
                // unchanged and, whenever G_NEED_RESCHED happens to be
                // clear, `trap_return` resumes the SAME faulting sepc —
                // an infinite illegal-instruction loop that also spams
                // the log forever (observed while investigating the SMP
                // idle-hart crash, todo.md). There is nothing to tear
                // down for pid 0, so halt this hart cleanly instead of
                // silently re-faulting; other harts are unaffected.
                if pid == 0 {
                    crate::srv::klog::halt();
                }
                proc::exit(pid, 132);
            }
            CAUSE_BRK => {
                let pid = proc::current_pid();
                if pid == 0 {
                    crate::srv::klog::halt();
                }
                proc::exit(pid, 133);
            }
            _ => {
                crate::kpanic!(
                    "trap",
                    "unhandled exception: hart=%d pid=%d scause=%p sepc=%p sp=%p ra=%p satp=%p",
                    onyx_core::fmt::Arg::from(crate::proc::process::hart_id() as u64),
                    onyx_core::fmt::Arg::from(proc::current_pid()),
                    onyx_core::fmt::Arg::from(scause),
                    onyx_core::fmt::Arg::from(tf.sepc),
                    onyx_core::fmt::Arg::from(tf.sp),
                    onyx_core::fmt::Arg::from(tf.ra),
                    onyx_core::fmt::Arg::from(crate::arch::csr::read_satp())
                );
            }
        }
    }
}

//! Trap dispatch.
use crate::arch::regs::*;
use crate::arch::trap_frame::TrapFrame;
use crate::drivers::plic;
use crate::proc;
use crate::srv::timer;
use crate::syscall::abi::SYS_sigreturn;
use crate::syscall::handler;

pub unsafe fn init() { unsafe {
    init_hart();
    crate::kinf!(
        "trap",
        "stvec=%p",
        onyx_core::fmt::Arg::from(crate::arch::asm::trap_entry as *const () as usize as u64)
    );
}}

pub unsafe fn init_hart() { unsafe {
    crate::arch::csr::write_stvec(crate::arch::asm::trap_entry as *const () as usize as u64);
    let hartid = crate::arch::smp::current_hart();
    let _ = hartid;
    // sscratch = 0 while in kernel mode (trap_entry uses it as the
    // user/kernel discriminator). drop_to_user sets it to the kernel stack
    // top right before entering user space.
    crate::arch::csr::write_sscratch(0);
}}

pub unsafe fn handle(tf: &mut TrapFrame) { unsafe {
    let scause = crate::arch::csr::read_scause();
    let is_int = scause & SCAUSE_INT != 0;
    let code = scause & !SCAUSE_INT;
    if is_int {
        match code {
            INTR_S_TIMER => timer::handle(),
            INTR_S_EXTERN => {
                plic::dispatch();
            }
            INTR_S_SOFT => {
                crate::kwrn!("trap", "unhandled S-soft interrupt");
            }
            _ => {
                crate::kwrn!(
                    "trap",
                    "unhandled interrupt: code=%d",
                    onyx_core::fmt::Arg::from(code)
                );
            }
        }
    } else {
        match code {
            CAUSE_U_ECALL => {
                // SYS_sigreturn fully restores the saved trap frame inside
                // the handler (it writes *tf = saved_tf). If we then go on
                // to overwrite a0 with the handler's return value and
                // advance sepc by 4, we corrupt the restored state — the
                // signal handler's return address and a0 would be lost.
                // Special-case sigreturn: skip the post-handle fixups.
                let is_sigreturn = tf.a7 == SYS_sigreturn;
                let ret = handler::handle(tf);
                if !is_sigreturn {
                    tf.a0 = ret as u64;
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
                        onyx_core::fmt::Arg::from(tf.a7 as u32)
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
                    "illegal instruction pid=%d sepc=%p",
                    onyx_core::fmt::Arg::from(pid),
                    onyx_core::fmt::Arg::from(tf.sepc)
                );
                proc::exit(pid, 132);
            }
            CAUSE_BRK => {
                let pid = proc::current_pid();
                proc::exit(pid, 133);
            }
            _ => {
                crate::kpanic!(
                    "trap",
                    "unhandled exception: scause=%p sepc=%p",
                    onyx_core::fmt::Arg::from(scause),
                    onyx_core::fmt::Arg::from(tf.sepc)
                );
            }
        }
    }
    // Signal delivery: check the current process for pending unblocked
    // signals. KILL terminates the process; other signals are cleared (MVP).
    proc::signal_check(tf);
    let pid = proc::current_pid();
    if pid != 0
        && let Some(p) = proc::by_pid(pid)
            && matches!(p.state, proc::ProcState::Exited) {
                proc::sched_yield(tf);
                // sched_yield returns only if it couldn't context-switch away.
                // For secondary harts, sched_yield switches to idle (never returns).
                // If we reach here, no runnable process exists — halt.
                crate::srv::klog::halt();
            }
    if proc::process::G_NEED_RESCHED[proc::process::hart_id()]
        .load(core::sync::atomic::Ordering::Acquire)
    {
        proc::sched_yield(tf);
    }
}}

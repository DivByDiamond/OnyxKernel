//! Trap dispatch.
//!
//! Split by responsibility (info.md rule 1/2): this module owns hart setup
//! and the top-level dispatcher; [`interrupts`] and [`exceptions`] hold the
//! per-cause handlers, and [`post`] holds the bookkeeping that runs after
//! every trap (signal delivery, stack-canary check, reschedule).
mod exceptions;
mod interrupts;
mod post;

use crate::arch::regs::*;
use crate::arch::trap_frame::TrapFrame;

/// Register the global trap entry point (logging only).
///
/// # Safety
///
/// Must run after kernel text/rodata are mapped and `trap_entry`
/// (assembly) is linked at its final address; requires S-mode execution.
pub unsafe fn init() {
    // SAFETY: `init_hart` only writes per-hart CSRs; valid on any hart in
    // S-mode per the contract above.
    unsafe {
        init_hart();
        crate::kinf!(
            "trap",
            "stvec=%p",
            onyx_core::fmt::Arg::from(crate::arch::asm::trap_entry as *const () as usize as u64)
        );
    }
}

/// Per-hart trap setup: point `stvec` at `trap_entry`, expose cycle/
/// instret/time counters to U-mode, and reset `sscratch` to 0 (kernel).
///
/// # Safety
///
/// Requires S-mode execution on the hart being configured, after that
/// hart's kernel stack/trap path is reachable from `trap_entry`.
pub unsafe fn init_hart() {
    // SAFETY: privileged CSR writes are valid in S-mode; the values match
    // what trap_asm expects (sscratch == 0 marks "trap from kernel").
    unsafe {
        crate::arch::csr::write_stvec(crate::arch::asm::trap_entry as *const () as usize as u64);
        // Enable cycle/instret/time for U-mode (S-mode access is gated by
        // mcounteren, set in the M-mode boot path / by the firmware).
        crate::arch::csr::write_scounteren(0x7);
        // Enable the FPU for user mode: sstatus.FS resets to Off (00), which
        // makes every floating-point instruction in U/S mode raise an
        // illegal-instruction trap. Hard-float binaries (anything built for
        // rv64gc, e.g. third-party vim.onx) die instantly without this.
        // The kernel itself never touches FP registers, so Initial (01)
        // suffices — no lazy FP context switching yet.
        crate::arch::csr::clear_sstatus(0xC000);
        crate::arch::csr::set_sstatus(0x4000);
        let hartid = crate::arch::smp::current_hart();
        let _ = hartid;
        // sscratch = 0 while in kernel mode (trap_entry uses it as the
        // user/kernel discriminator). drop_to_user sets it to the kernel stack
        // top right before entering user space.
        crate::arch::csr::write_sscratch(0);
    }
}

/// Top-level trap dispatcher: services interrupts (timer/external/soft),
/// syscalls, page faults and illegal instructions against the saved frame,
/// then runs signal delivery, the kernel-stack canary check and the
/// reschedule hooks.
///
/// # Safety
///
/// `tf` must be the live, exclusively-owned trap frame pushed by
/// `trap_entry` on this hart; must run in S-mode with SIE cleared (the
/// spinlocks taken by handlers rely on it). May never return for exited
/// processes (context-switches away or halts).
pub unsafe fn handle(tf: &mut TrapFrame) {
    // SAFETY: CSR reads and the volatile canary read below are valid on a
    // trapped hart; `tf` exclusivity is guaranteed per the contract above.
    unsafe {
        let scause = crate::arch::csr::read_scause();
        // Captured once, up front: whether THIS trap interrupted user mode
        // (SPP=0) or kernel mode (SPP=1). Used at the bottom to decide
        // whether the opportunistic reschedule below is safe to act on —
        // see the comment there.
        let interrupted_user = crate::arch::csr::read_sstatus() & SSTATUS_SPP == 0;
        let is_int = scause & SCAUSE_INT != 0;
        let code = scause & !SCAUSE_INT;
        if is_int {
            interrupts::handle(code);
        } else {
            exceptions::handle(tf, scause, code);
        }
        post::run(tf, scause, interrupted_user);
    }
}

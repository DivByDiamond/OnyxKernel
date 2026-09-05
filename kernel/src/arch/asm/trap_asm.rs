//! trap.S — trap entry/return, `sched_switch`, and `drop_to_user`. Exposes
//! the asm-implemented symbols `trap_entry`, `trap_return`, `sched_switch`,
//! `drop_to_user` via `extern "Rust"` decls.
use core::arch::global_asm;

// ─── trap.S ──────────────────────────────────────────────────────────────────
global_asm!(
    r#"
.section .text.trap
.balign 4
.global trap_entry
trap_entry:
    csrrw sp, sscratch, sp
    bnez sp, .Ltrap_from_user
    csrr sp, sscratch
.Ltrap_from_user:
    addi sp, sp, -288
    sd t0, 32(sp)
    csrr t0, sscratch
    sd t0, 8(sp)
    csrw sscratch, zero
    sd ra, 0(sp)
    sd gp, 16(sp)
    sd tp, 24(sp)
    sd t1, 40(sp)
    sd t2, 48(sp)
    sd s0, 56(sp)
    sd s1, 64(sp)
    sd a0, 72(sp)
    sd a1, 80(sp)
    sd a2, 88(sp)
    sd a3, 96(sp)
    sd a4, 104(sp)
    sd a5, 112(sp)
    sd a6, 120(sp)
    sd a7, 128(sp)
    sd s2, 136(sp)
    sd s3, 144(sp)
    sd s4, 152(sp)
    sd s5, 160(sp)
    sd s6, 168(sp)
    sd s7, 176(sp)
    sd s8, 184(sp)
    sd s9, 192(sp)
    sd s10, 200(sp)
    sd s11, 208(sp)
    sd t3, 216(sp)
    sd t4, 224(sp)
    sd t5, 232(sp)
    sd t6, 240(sp)
    li t0, (1 << 18)
    csrs sstatus, t0
    csrr t0, sepc
    sd t0, 248(sp)
    csrr t0, sstatus
    sd t0, 256(sp)
    csrr t0, satp
    sd t0, 280(sp)
    mv a0, sp
    call trap_handler

.global trap_return
trap_return:
    ld ra, 0(sp)
    ld gp, 16(sp)
    // Root-cause fix (SMP crash, todo.md "Отдельный SMP-краш под
    // -smp 2"): tp is this kernel's hart-id register (hart_id() is a raw
    // `mv <reg>, tp` read everywhere), not an ordinary per-context register
    // — it must always equal the PHYSICAL hart currently executing, never
    // whatever a trapframe happened to capture. Restoring it from the
    // trapframe here was wrong for two reasons: (1) a freshly created
    // process's trapframe (proc::spawn::create_user) never sets tf.tp, so
    // it defaults to 0 (TrapFrame::zero()) — its first-ever resume would
    // zero tp on whichever hart happened to run it; (2) a process
    // migrated between harts (work-stealing) carries the tp value from
    // whichever hart it last ran on, not the one resuming it now. Simply
    // never touching tp here is correct: at this point we are still
    // running as kernel code on the ACTUAL physical hart, so tp already
    // holds the right value — leave it alone instead of overwriting it
    // with a stale or defaulted one. (See the matching drop_to_user fix
    // in this file for the other half of this bug.)
    //
    // Root-cause fix (KDF/hash_password nondeterminism under long-running
    // user loops, 2026-09-05): t0/t1 used to be restored to their REAL
    // (interrupted-context) values here, immediately after ra/gp — but the
    // CSR bookkeeping further down (sepc/sstatus/satp/final-sp) then
    // reused t0/t1 as scratch registers and never reloaded the real
    // values before `sret`. Every single timer tick that interrupted user
    // code got its t0/t1 silently replaced with leftover CSR-scratch
    // garbage on resume — invisible for most code, but any register-heavy
    // hot loop (e.g. the password KDF's SHA-256 core, which keeps working
    // state in temporaries across thousands of iterations) would
    // eventually get preempted mid-loop and resume with corrupted t0/t1,
    // silently producing a wrong result with no fault or crash. Fixed by
    // moving the real t0/t1 restore to the very end, after all CSR
    // scratch work is done, and folding the final sp swap into a single
    // `ld sp, 8(sp)` (address computed from the OLD sp before the load
    // lands in sp) so no scratch register is needed for it at all.
    ld t2, 48(sp)
    ld s0, 56(sp)
    ld s1, 64(sp)
    ld a0, 72(sp)
    ld a1, 80(sp)
    ld a2, 88(sp)
    ld a3, 96(sp)
    ld a4, 104(sp)
    ld a5, 112(sp)
    ld a6, 120(sp)
    ld a7, 128(sp)
    ld s2, 136(sp)
    ld s3, 144(sp)
    ld s4, 152(sp)
    ld s5, 160(sp)
    ld s6, 168(sp)
    ld s7, 176(sp)
    ld s8, 184(sp)
    ld s9, 192(sp)
    ld s10, 200(sp)
    ld s11, 208(sp)
    ld t3, 216(sp)
    ld t4, 224(sp)
    ld t5, 232(sp)
    ld t6, 240(sp)
    // From here to the real t0/t1 restore below, t0/t1 hold only CSR
    // scratch values — never the interrupted context's real registers.
    ld t0, 248(sp)
    csrw sepc, t0
    // Restore sstatus with SIE (bit 1) force-cleared: ALL trap-handler and
    // scheduler code runs with interrupts off, which is what makes the
    // kernel's SpinLocks safe (see crate::sync). Interrupts are turned back
    // on at exactly two kinds of points, never here:
    //   * Return to USER mode: user trap frames carry sstatus.SPIE = 1
    //     (set by drop_to_user / spawn / fork / sigreturn frame fixups), so
    //     the sret below performs SPIE -> SIE in hardware as it drops to
    //     U-mode.
    //   * The idle loop (proc/scheduler/idle.rs) re-enables SIE itself right
    //     after re-arming its timer, immediately before wfi, holding no
    //     locks. Kernel-mode returns (SPP = 1, SPIE = old SIE = 0) resume
    //     with interrupts still off.
    ld t0, 256(sp)
    li t1, ~(1 << 1)
    and t0, t0, t1
    csrw sstatus, t0
    srli t0, t0, 8
    andi t0, t0, 1
    bnez t0, .Lret_kernel
    addi t0, sp, 288
    csrw sscratch, t0
    j .Lret_finish
.Lret_kernel:
    csrw sscratch, zero
.Lret_finish:
    ld t0, 280(sp)
    csrw satp, t0
    sfence.vma zero, zero
    // Real t0/t1 restored last — nothing after this touches them except
    // the final sp swap, which uses sp itself (not t0/t1) as scratch.
    ld t1, 40(sp)
    ld t0, 32(sp)
    ld sp, 8(sp)
    sret

.global sched_switch
sched_switch:
    mv sp, a0
    j trap_return

.global drop_to_user
drop_to_user:
    csrw sscratch, sp
    li t0, (1 << 1) | (1 << 8)
    csrc sstatus, t0
    li t0, (1 << 5) | (1 << 18)
    csrs sstatus, t0
    li t0, (1 << 1) | (1 << 9)
    csrs sie, t0
    li t0, (8 << 60)
    srli t1, a2, 12
    or t0, t0, t1
    csrw satp, t0
    sfence.vma zero, zero
    csrw sepc, a0
    mv sp, a1
    li a0, 0
    li a1, 0
    li a2, 0
    li a3, 0
    li a4, 0
    li a5, 0
    li a6, 0
    li a7, 0
    li t0, 0
    li t1, 0
    li t2, 0
    li t3, 0
    li t4, 0
    li t5, 0
    li t6, 0
    // Bug (syscall MINOR #5): zero the callee-saved registers s0-s11
    // before entering user space. The previous code left whatever values
    // the kernel had in these registers — a user program reading them
    // (e.g. via inline asm) would see kernel stack pointers, frame
    // pointers, and other sensitive data. Zero them all so user space
    // starts with a clean register state.
    li s0, 0
    li s1, 0
    li s2, 0
    li s3, 0
    li s4, 0
    li s5, 0
    li s6, 0
    li s7, 0
    li s8, 0
    li s9, 0
    li s10, 0
    li s11, 0
    li gp, 0
    // Root-cause fix (SMP crash, todo.md "Отдельный SMP-краш под
    // -smp 2"): `tp` is NOT an ordinary callee-saved register in this
    // kernel — every hart's `hart_id()` is a raw `mv <reg>, tp` read, and
    // the entire scheduler (G_HART_CURRENT, runqueues, idle contexts,
    // UART_LOCK ownership, ...) trusts it identifies which physical hart
    // is executing. Zeroing it here (as part of the syscall MINOR #5
    // register-hygiene fix above) meant every process ran in user mode
    // with tp=0, and the NEXT trap on THAT hart — any syscall, timer
    // tick, page fault — entered the kernel with tp still 0, regardless
    // of which physical hart actually took the trap. Confirmed live: hart
    // 1 running user code and trapping back in would be treated as hart 0
    // by every hart-indexed lookup, corrupting shared per-"hart 0" state
    // (G_HART_CURRENT[0], its runqueue, G_HART_IDLE_TF[0], ...) — the
    // deepest layer of the SMP crash chased across this investigation.
    // No userspace code in this project's toolchain (libonyxc/onyxcc)
    // reads tp for anything (checked: no TLS or other use), so leaving
    // the hart id in it while in user mode leaks nothing meaningful — a
    // small integer, unlike the kernel stack pointers/addresses the other
    // zeroed registers above protect against. `gp` has no such
    // significance to the kernel and stays zeroed.
    sret
"#,
);

// SAFETY: the symbols are defined by the global_asm! above (trap.S); the
// declared signatures match the asm ABI (a0 = new_sp for sched_switch;
// a0-a2 = entry/ustack/user_root_pa for drop_to_user).
unsafe extern "Rust" {
    pub fn trap_entry();
    pub fn trap_return();
    pub fn sched_switch(new_sp: usize) -> !;
    pub fn drop_to_user(entry: usize, ustack: usize, user_root_pa: usize) -> !;
}

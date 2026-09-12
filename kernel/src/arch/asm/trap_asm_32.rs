//! trap.S (32-bit) — trap entry/return, sched_switch, drop_to_user.
//! 32-bit version: uses sw/lw instead of sd/ld, half offsets, Sv32 SATP.
//!
//! Root-cause fix (2026-09-12, found live-testing the rv32 timer port —
//! todo.md "rv32 mtrap"): this file used to be a mechanical `sd`->`sw`/
//! `ld`->`lw` translation of `trap_asm.rs` that kept the RV64 byte offsets
//! (0, 8, 16, 24, 32, ... stride 8) unchanged instead of halving them to
//! match the 32-bit `TrapFrame`'s real 4-byte-per-field stride (0, 4, 8,
//! 12, 16, ... — see `arch::trap_frame::TrapFrame`, `TRAP_FRAME_SIZE=144`
//! on rv32). Every field past `ra` (offset 0, correct only by accident)
//! landed in the WRONG slot, and the highest offset used (280) is 136
//! bytes past the actual 144-byte frame `addi sp, sp, -144` reserved —
//! every single trap silently smashed whatever the kernel stack held
//! above the frame. It also dropped the kernel-vs-user discrimination
//! entirely (rv64's `bnez sp, .Ltrap_from_user` / `.Lret_kernel` SPP
//! check): with `sscratch == 0` marking "trap from kernel" (see
//! `srv::trap::init`), `csrrw sp, sscratch, sp` on a kernel-mode trap left
//! `sp == 0`, and `addi sp, sp, -144` wrapped to `0xFFFFFF70` — a wild
//! stack pointer used for every subsequent save. This was invisible until
//! this session because rv32 had never taken a real S-mode trap in a live
//! boot before (see the vmm/Sv32 fixes in the same commit series — rv32
//! literally never booted far enough for any of this to run). Both bugs
//! together produced the exact observed symptom: a "kernel page fault"
//! with `sepc` pointing INSIDE the stack region, i.e. the CPU executing
//! whatever garbage the corrupted save routine had just written there.
use core::arch::global_asm;

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
    addi sp, sp, -144
    sw t0, 16(sp)
    csrr t0, sscratch
    sw t0, 4(sp)
    csrw sscratch, zero
    sw ra, 0(sp)
    sw gp, 8(sp)
    sw tp, 12(sp)
    sw t1, 20(sp)
    sw t2, 24(sp)
    sw s0, 28(sp)
    sw s1, 32(sp)
    sw a0, 36(sp)
    sw a1, 40(sp)
    sw a2, 44(sp)
    sw a3, 48(sp)
    sw a4, 52(sp)
    sw a5, 56(sp)
    sw a6, 60(sp)
    sw a7, 64(sp)
    sw s2, 68(sp)
    sw s3, 72(sp)
    sw s4, 76(sp)
    sw s5, 80(sp)
    sw s6, 84(sp)
    sw s7, 88(sp)
    sw s8, 92(sp)
    sw s9, 96(sp)
    sw s10, 100(sp)
    sw s11, 104(sp)
    sw t3, 108(sp)
    sw t4, 112(sp)
    sw t5, 116(sp)
    sw t6, 120(sp)
    li t0, (1 << 18)
    csrs sstatus, t0
    csrr t0, sepc
    sw t0, 124(sp)
    csrr t0, sstatus
    sw t0, 128(sp)
    csrr t0, satp
    sw t0, 140(sp)
    mv a0, sp
    call trap_handler

.global trap_return
trap_return:
    lw ra, 0(sp)
    lw gp, 8(sp)
    // See the matching, fully-explained fix in trap_asm.rs (rv64) for both
    // of the following: never restore tp from the trapframe (tp is this
    // kernel's hart-id register — the live value is always correct, a
    // trapframe's captured value is not); and t0/t1 must not be restored
    // to their real values until after all CSR-scratch use below, or every
    // timer tick silently corrupts whatever the interrupted code was
    // keeping in them.
    lw t2, 24(sp)
    lw s0, 28(sp)
    lw s1, 32(sp)
    lw a0, 36(sp)
    lw a1, 40(sp)
    lw a2, 44(sp)
    lw a3, 48(sp)
    lw a4, 52(sp)
    lw a5, 56(sp)
    lw a6, 60(sp)
    lw a7, 64(sp)
    lw s2, 68(sp)
    lw s3, 72(sp)
    lw s4, 76(sp)
    lw s5, 80(sp)
    lw s6, 84(sp)
    lw s7, 88(sp)
    lw s8, 92(sp)
    lw s9, 96(sp)
    lw s10, 100(sp)
    lw s11, 104(sp)
    lw t3, 108(sp)
    lw t4, 112(sp)
    lw t5, 116(sp)
    lw t6, 120(sp)
    // From here to the real t0/t1 restore below, t0/t1 hold only CSR
    // scratch values — never the interrupted context's real registers.
    lw t0, 124(sp)
    csrw sepc, t0
    // Restore sstatus with SIE (bit 1) force-cleared — see trap_asm.rs's
    // fully-explained comment: all trap-handler/scheduler code runs with
    // interrupts off (crate::sync's SpinLock invariant); SIE only comes
    // back via sret's SPIE->SIE hardware transition (user) or the idle
    // loop (kernel), never here.
    lw t0, 128(sp)
    li t1, ~(1 << 1)
    and t0, t0, t1
    csrw sstatus, t0
    // SPP (bit 8) tells us which mode we are RETURNING to: 1 = kernel
    // (this trap's sscratch must go back to 0, matching srv::trap's
    // "sscratch == 0 means kernel" convention for the entry-side check
    // above), 0 = user (sscratch must point past this frame so the NEXT
    // trap-from-user knows where the kernel stack is). Missing this
    // (unconditionally pointing sscratch past the frame either way) was
    // the second half of the bug this file used to have: it silently
    // broke the entry-side kernel/user discrimination on every return.
    srli t0, t0, 8
    andi t0, t0, 1
    bnez t0, .Lret_kernel
    addi t0, sp, 144
    csrw sscratch, t0
    j .Lret_finish
.Lret_kernel:
    csrw sscratch, zero
.Lret_finish:
    lw t0, 140(sp)
    csrw satp, t0
    sfence.vma zero, zero
    // Real t0/t1 restored last; the final sp swap uses sp itself as
    // scratch (address computed from the OLD sp before the load lands).
    lw t1, 20(sp)
    lw t0, 16(sp)
    lw sp, 4(sp)
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
    li t0, (1 << 31)
    srli t1, a2, 12
    // PPN mask for Sv32 satp (26 bits) — too wide for `andi`, use a reg.
    li t2, 0x3FFFFF
    and t1, t1, t2
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
    // Root-cause fix (SMP crash, OnyxKernel todo.md "Отдельный SMP-краш
    // под -smp 2"): keep `tp` (this kernel's hart-id register — see the
    // matching comment in trap_asm.rs) instead of zeroing it here. No
    // userspace code reads tp for anything, so this leaks nothing
    // meaningful; `gp` stays zeroed as before.
    sret
"#,
);

// SAFETY: the symbols are defined by the global_asm! above (trap.S, 32-bit);
// the declared signatures match the asm ABI (a0 = new_sp for sched_switch;
// a0-a2 = entry/ustack/user_root_pa for drop_to_user).
unsafe extern "Rust" {
    pub fn trap_entry();
    pub fn trap_return();
    pub fn sched_switch(new_sp: usize) -> !;
    pub fn drop_to_user(entry: usize, ustack: usize, user_root_pa: usize) -> !;
}

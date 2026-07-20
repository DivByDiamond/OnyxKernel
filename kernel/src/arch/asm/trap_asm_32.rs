//! trap.S (32-bit) — trap entry/return, sched_switch, drop_to_user.
//! 32-bit version: uses sw/lw instead of sd/ld, half offsets, Sv32 SATP.
use core::arch::global_asm;

global_asm!(
    r#"
.section .text.trap
.balign 4
.global trap_entry
trap_entry:
    csrrw sp, sscratch, sp
    addi sp, sp, -144
    sw t0, 32(sp)
    csrr t0, sscratch
    sw t0, 8(sp)
    sw ra, 0(sp)
    sw gp, 16(sp)
    sw tp, 24(sp)
    sw t1, 40(sp)
    sw t2, 48(sp)
    sw s0, 56(sp)
    sw s1, 64(sp)
    sw a0, 72(sp)
    sw a1, 80(sp)
    sw a2, 88(sp)
    sw a3, 96(sp)
    sw a4, 104(sp)
    sw a5, 112(sp)
    sw a6, 120(sp)
    sw a7, 128(sp)
    sw s2, 136(sp)
    sw s3, 144(sp)
    sw s4, 152(sp)
    sw s5, 160(sp)
    sw s6, 168(sp)
    sw s7, 176(sp)
    sw s8, 184(sp)
    sw s9, 192(sp)
    sw s10, 200(sp)
    sw s11, 208(sp)
    sw t3, 216(sp)
    sw t4, 224(sp)
    sw t5, 232(sp)
    sw t6, 240(sp)
    li t0, (1 << 18)
    csrs sstatus, t0
    csrr t0, sepc
    sw t0, 248(sp)
    csrr t0, sstatus
    sw t0, 256(sp)
    csrr t0, satp
    sw t0, 280(sp)
    mv a0, sp
    call trap_handler

.global trap_return
trap_return:
    lw ra, 0(sp)
    lw gp, 16(sp)
    lw tp, 24(sp)
    lw t0, 32(sp)
    lw t1, 40(sp)
    lw t2, 48(sp)
    lw s0, 56(sp)
    lw s1, 64(sp)
    lw a0, 72(sp)
    lw a1, 80(sp)
    lw a2, 88(sp)
    lw a3, 96(sp)
    lw a4, 104(sp)
    lw a5, 112(sp)
    lw a6, 120(sp)
    lw a7, 128(sp)
    lw s2, 136(sp)
    lw s3, 144(sp)
    lw s4, 152(sp)
    lw s5, 160(sp)
    lw s6, 168(sp)
    lw s7, 176(sp)
    lw s8, 184(sp)
    lw s9, 192(sp)
    lw s10, 200(sp)
    lw s11, 208(sp)
    lw t3, 216(sp)
    lw t4, 224(sp)
    lw t5, 232(sp)
    lw t6, 240(sp)
    lw t0, 248(sp)
    csrw sepc, t0
    lw t0, 256(sp)
    li t1, ~(1 << 1)
    and t0, t0, t1
    csrw sstatus, t0
    addi t1, sp, 144
    csrw sscratch, t1
    lw t0, 8(sp)
    lw t1, 280(sp)
    csrw satp, t1
    sfence.vma zero, zero
    mv sp, t0
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
    andi t1, t1, 0x3FFFFF
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
    li tp, 0
    sret
"#,
);

unsafe extern "Rust" {
    pub fn trap_entry();
    pub fn trap_return();
    pub fn sched_switch(new_sp: usize) -> !;
    pub fn drop_to_user(entry: usize, ustack: usize, user_root_pa: usize) -> !;
}

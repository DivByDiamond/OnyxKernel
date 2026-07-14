//! boot.S — kernel entry point. Sets up PMP, medeleg/mideleg, switches to
//! supervisor mode, and `mret`s into `kmain`. Parks non-boot harts in `wfi`.
use crate::arch::{__bss_end, __bss_start, __stack_top, SAVED_FDT, SAVED_HARTID};
use core::arch::global_asm;

// ─── boot.S ──────────────────────────────────────────────────────────────────
global_asm!(
    r#"
.section .text.boot
.global _start
_start:
    csrr tp, mhartid
    bnez tp, park
    la t0, {saved_hartid}
    sd tp, 0(t0)
    la t0, {saved_fdt}
    sd a1, 0(t0)
    la t0, {bss_start}
    la t1, {bss_end}
1:  bgeu t0, t1, 2f
    sd zero, 0(t0)
    addi t0, t0, 8
    j 1b
2:
    la sp, {stack_top}
    li t0, 0x3FFFFFFF
    csrw pmpaddr0, t0
    li t0, 0x9F
    csrw pmpcfg0, t0
    // Delegate the standard set of S-mode exceptions, INCLUDING:
    //   bit 0  — instruction misaligned
    //   bit 1  — instruction access fault
    //   bit 2  — illegal instruction
    //   bit 3  — breakpoint
    //   bit 5  — load access fault
    //   bit 7  — store access fault
    //   bit 8  — environment call from U-mode
    //   bit 9  — environment call from S-mode
    //   bit 11 — instruction page fault
    //   bit 12 — load page fault
    //   bit 13 — store page fault
    //   bit 15 — store/AMO page fault
    //
    // Bug (syscall MINOR #6, #7, #8): also delegate misaligned access
    // faults (bits 0, 4, 6) so that user-space misaligned loads/stores
    // trap into S-mode and can be handled (or kill the process) instead
    // of crashing the machine in M-mode. Reserved bits in medeleg are
    // Read-Only-Zero — writing 1 to them is a no-op, but we avoid them
    // to be a good citizen.
    //
    // Without bits 1 and 11 a jump into unmapped or unmapped-execute
    // memory traps into M-mode and hangs/crashes the machine instead of
    // being delivered to the kernel as an S-mode page fault.
    li t0, (1<<0)|(1<<1)|(1<<2)|(1<<3)|(1<<5)|(1<<7)|(1<<8)|(1<<9)|(1<<11)|(1<<12)|(1<<13)|(1<<15)
    csrw medeleg, t0
    li t0, (1<<1)|(1<<5)|(1<<9)
    csrw mideleg, t0
    csrw mie, zero
    li t0, (1<<11)
    csrs mstatus, t0
    li t0, (1<<7)
    csrc mstatus, t0
    la t0, kmain
    csrw mepc, t0
    la t0, {saved_hartid}
    ld a0, 0(t0)
    la t0, {saved_fdt}
    ld a1, 0(t0)
    csrw satp, zero
    mret
park:
    la t0, secondary_entry
    jr t0
"#,
    saved_hartid = sym SAVED_HARTID,
    saved_fdt = sym SAVED_FDT,
    bss_start = sym __bss_start,
    bss_end = sym __bss_end,
    stack_top = sym __stack_top,
);

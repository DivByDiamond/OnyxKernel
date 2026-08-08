//! boot_smode.S — kernel entry point for S-mode (OC2R / OpenSBI fw_jump).
//!
//! OpenSBI enters `_start` directly in S-mode with `a0 = hartid` and
//! `a1 = DTB pointer`. Unlike boot.rs (M-mode), OpenSBI has already done
//! the low-level setup for us: PMP, exception/interrupt delegation
//! (medeleg/mideleg) and the initial paging state. So this file MUST NOT
//! touch any M-mode CSR (no pmp*, medeleg/mideleg, mie/mip, mepc, mret,
//! satp write) — doing so would trap in S-mode. We just park secondary
//! harts, save hartid/DTB, zero BSS, set the stack, and jump straight
//! into `kmain`.
use crate::arch::{__bss_end, __bss_start, __stack_top, SAVED_FDT, SAVED_HARTID};
use core::arch::global_asm;

// ─── boot_smode.S ────────────────────────────────────────────────────────────
#[cfg(not(test))]
global_asm!(
    r#"
.section .text.boot
.global _start

// Debug marker: write one byte to the console (UART data reg @ 0x10000000).
// Bounded poll on LSR so an absent console can't hang the boot. Used only to
// trace where the S-mode entry gets to on a new board; harmless otherwise.
.macro uart_mark, c
    li t2, 0x10000000
    li t3, 0
1:  addi t3, t3, 1
    li t4, 0x8000
    bgeu t3, t4, 2f
    lb t4, 5(t2)
    andi t4, t4, 0x20
    beqz t4, 1b
2:  li t4, \c
    sb t4, 0(t2)
.endm

_start:
    uart_mark 'O'                 // entered from OpenSBI
    mv tp, a0                  // OpenSBI hands us hartid in a0; kernel reads hartid from tp
    bnez tp, park
    la t0, {saved_hartid}
    sd a0, 0(t0)
    la t0, {saved_fdt}
    sd a1, 0(t0)
    uart_mark 'H'                 // hartid + dtb saved
    la t0, {bss_start}
    la t1, {bss_end}
1:  bgeu t0, t1, 2f
    sd zero, 0(t0)
    addi t0, t0, 8
    j 1b
2:
    uart_mark 'B'                 // BSS cleared
    la sp, {stack_top}
    uart_mark 'S'                 // stack set
    la t0, kmain
    jalr t0                    // a0=hartid, a1=dtb preserved through BSS clear/stack setup
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

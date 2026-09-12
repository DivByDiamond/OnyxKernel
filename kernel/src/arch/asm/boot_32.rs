//! boot.S — kernel entry point for rv32gc (Sv32 paging).
//!
//! 32-bit version of boot.rs. Differences from the 64-bit version:
//!   - Uses `sw`/`lw` instead of `sd`/`ld` (registers are 32-bit).
//!   - PMP address covers 32-bit address space (0x3FFFFFFF is still valid).
//!   - SATP mode for Sv32 is bit 31 (0x80000000), set in install_root()
//!     not here — boot runs in M-mode with paging off.
//!   - `li` for large constants uses different encoding on rv32 (no LUI+ADDI
//!     pair for 64-bit values).
//!
//! This file is compiled only when target_pointer_width = "32".
use crate::arch::asm::mtrap_32::G_MTRAP_SCRATCH_32;
use crate::arch::{__bss_end, __bss_start, __stack_top, SAVED_FDT, SAVED_HARTID};
use core::arch::global_asm;

global_asm!(
    r#"
.section .text.boot
.global _start
_start:
    csrr tp, mhartid
    bnez tp, park
    la t0, {saved_hartid}
    sw tp, 0(t0)
    la t0, {saved_fdt}
    sw a1, 0(t0)
    la t0, {bss_start}
    la t1, {bss_end}
1:  bgeu t0, t1, 2f
    sw zero, 0(t0)
    addi t0, t0, 4
    j 1b
2:
    la sp, {stack_top}
    li t0, 0x3FFFFFFF
    csrw pmpaddr0, t0
    li t0, 0x9F
    csrw pmpcfg0, t0
    // Delegate the same set of S-mode exceptions as the 64-bit version.
    // Bit 9 (S-mode ecall) is deliberately NOT delegated — see boot.rs's
    // medeleg comment: mtrap_entry_32 (arch/asm/mtrap_32.rs) catches it as
    // the legacy SBI_SET_TIMER call this hart's srv::timer code issues to
    // arm its own next tick (same MTIP->STIP forwarding fix as rv64,
    // ported 2026-09-12 — see todo.md).
    li t0, (1<<0)|(1<<1)|(1<<2)|(1<<3)|(1<<5)|(1<<7)|(1<<8)|(1<<11)|(1<<12)|(1<<13)|(1<<15)
    csrw medeleg, t0
    li t0, (1<<1)|(1<<5)|(1<<9)
    csrw mideleg, t0
    // Allow S-mode (and via scounteren later U-mode) to read cycle/time/
    // instret. Without this every `csrr cycle`/rdcycle (e.g.
    // drivers::entropy::hwrand::next_u32) from S-mode raises an
    // illegal-instruction trap (mcounteren defaults to 0) — found
    // 2026-09-12 live-testing the rv32 timer port: boot.rs (rv64) and
    // BOTH branches of arch/smp/secondary.rs already set this, but
    // boot_32.rs (hart 0's rv32 entry) never did.
    li t0, (1<<0)|(1<<1)|(1<<2)
    csrw mcounteren, t0
    // mscratch -> this hart's row of G_MTRAP_SCRATCH_32 (hart 0, tp=0, so
    // the row is at the array's base address — no offset needed); mtvec ->
    // mtrap_entry_32. Both must be live before mie/MTIE is ever enabled
    // (the first arch::sbi::set_timer call, from srv::timer::init()).
    la t0, {mtrap_scratch}
    csrw mscratch, t0
    la t0, mtrap_entry_32
    csrw mtvec, t0
    csrw mie, zero
    li t0, (1<<11)
    csrs mstatus, t0
    li t0, (1<<7)
    csrc mstatus, t0
    la t0, kmain
    csrw mepc, t0
    la t0, {saved_hartid}
    lw a0, 0(t0)
    la t0, {saved_fdt}
    lw a1, 0(t0)
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
    mtrap_scratch = sym G_MTRAP_SCRATCH_32,
);

//! M-mode trap handler for the non-`smode` rv32 boot chain — 32-bit mirror
//! of `mtrap.rs` (see that file's doc comment for the full MTIP→STIP
//! root-cause story; this file only documents what differs for rv32).
//!
//! Ported 2026-09-12 (todo.md "rv32 mtrap" follow-up): `boot_32.rs` had the
//! exact same M→S bug as `boot.rs` did before `mtrap.rs` — a single `mret`
//! into S-mode with `mie` zeroed and never a return to M-mode, so the CLINT
//! `mtimecmp` comparator's `mip.MTIP` (M-mode-only, never delegable) had
//! nothing to forward it to `mip.STIP`. Same fix, same two causes:
//!
//!   - `ecall` from S-mode (cause 9): on rv64 the whole 64-bit `stime`
//!     value fits in `a0`. On rv32, registers are 32 bits wide — the
//!     legacy v0.1 SBI_SET_TIMER ABI splits it across TWO real input
//!     registers, `a0` (low 32 bits) and `a1` (high 32 bits); see
//!     `arch::sbi::set_timer`'s rv32 variant, which this handler must
//!     agree with byte-for-byte. Both halves get read directly off the
//!     live (not-yet-clobbered) registers and written straight into the
//!     calling hart's CLINT `mtimecmp` slot.
//!   - Machine timer interrupt (cause 7) and the cross-hart IPI doorbell
//!     (cause 3, MSI — used by `destroy_root`'s TLB shootdown broadcast,
//!     see `arch::smp`) are otherwise identical to the rv64 handler: same
//!     CSR bit positions (XLEN-independent), same CLINT MSIP address
//!     arithmetic (bug-for-bug consistent with `mtrap.rs`'s `.Lm_soft_fwd`
//!     — not something this port changes).
use core::arch::global_asm;

/// Per-hart save area for `t1`/`t2`/`a0`/`a1`/`a7` — one more slot than the
/// rv64 scratch (`mtrap.rs`'s `G_MTRAP_SCRATCH`) because the ecall path here
/// needs BOTH `a0` and `a1` as real inputs (the 64-bit stime split), not
/// just `a0`. `a7` carries the SBI extension id for the base/SRST dispatch,
/// `a0`/`a1` double as the ecall output slots (same protocol as rv64).
/// Indexed by `mhartid`; `mscratch` on each hart points at its own row (set
/// once per hart at M-mode boot, before that hart's first possible M-mode
/// trap — `boot_32.rs` / `arch/smp/secondary.rs`).
#[unsafe(no_mangle)]
pub static mut G_MTRAP_SCRATCH_32: [[u32; 5]; crate::arch::smp::MAX_HARTS] =
    [[0; 5]; crate::arch::smp::MAX_HARTS];

global_asm!(
    r#"
.section .text.boot
.balign 4
.global mtrap_entry_32
mtrap_entry_32:
    csrrw t0, mscratch, t0
    sw t1, 0(t0)
    sw t2, 4(t0)
    sw a0, 8(t0)
    sw a1, 12(t0)
    sw a7, 16(t0)
    csrr t1, mcause
    bltz t1, .Lm32_interrupt
    // Exception: only ecall-from-S-mode (medeleg leaves every other
    // exception delegated straight to S-mode) should ever reach here.
    li t2, 9
    bne t1, t2, .Lm32_restore
    csrr t2, mepc
    addi t2, t2, 4
    csrw mepc, t2
    // Dispatch on the SBI extension id in a7 (mirrors mtrap.rs).
    li t2, 0
    beq a7, t2, .Lm32_set_timer
    li t2, 0x10
    beq a7, t2, .Lm32_base
    li t2, 0x53525354
    beq a7, t2, .Lm32_srst
    li t2, -2
    sw t2, 8(t0)
    sw zero, 12(t0)
    j .Lm32_restore
.Lm32_set_timer:
    csrr t2, mhartid
    slli t2, t2, 3
    li t1, 0x02004000
    add t1, t1, t2
    sw a0, 0(t1)
    sw a1, 4(t1)
    li t2, 0x20
    csrc mip, t2
    li t2, 0x80
    csrs mie, t2
    j .Lm32_restore
.Lm32_base:
    li t2, 0
    beq a6, t2, .Lm32_base_version
    li t2, 1
    beq a6, t2, .Lm32_base_impl_id
    li t2, 2
    beq a6, t2, .Lm32_base_impl_version
    li t2, 3
    beq a6, t2, .Lm32_base_probe
    j .Lm32_not_supported
.Lm32_base_version:
    sw zero, 8(t0)
    li t1, 0x02000000
    sw t1, 12(t0)
    j .Lm32_restore
.Lm32_base_impl_id:
    sw zero, 8(t0)
    li t1, 0x4F4E5958
    sw t1, 12(t0)
    j .Lm32_restore
.Lm32_base_impl_version:
    sw zero, 8(t0)
    li t1, 1
    sw t1, 12(t0)
    j .Lm32_restore
.Lm32_base_probe:
    li t1, 0x10
    beq a0, t1, .Lm32_probe_yes
    li t1, 0x53525354
    beq a0, t1, .Lm32_probe_yes
    sw zero, 8(t0)
    sw zero, 12(t0)
    j .Lm32_restore
.Lm32_probe_yes:
    sw zero, 8(t0)
    li t1, 1
    sw t1, 12(t0)
    j .Lm32_restore
.Lm32_not_supported:
    li t1, -2
    sw t1, 8(t0)
    sw zero, 12(t0)
    j .Lm32_restore
.Lm32_srst:
    li t1, 0
    bne a6, t1, .Lm32_not_supported
    sw zero, 8(t0)
    sw zero, 12(t0)
    li t1, 0x100000
    li t2, 0x5555
    beqz a0, 1f
    li t2, 0x7777
1:  sw t2, 0(t1)
    j .Lm32_restore
.Lm32_interrupt:
    andi t2, t1, 0xf
    li t1, 7
    beq t2, t1, .Lm32_timer_fwd
    li t1, 3
    beq t2, t1, .Lm32_soft_fwd
    j .Lm32_restore
.Lm32_timer_fwd:
    li t2, 0x80
    csrc mie, t2
    li t2, 0x20
    csrs mip, t2
    j .Lm32_restore
.Lm32_soft_fwd:
    // Cross-hart IPI doorbell (destroy_root TLB broadcast): MSIP raised by
    // the sending hart's CLINT write is already visible in mip.MSIP; clear
    // MSIP at the source (CLINT MMIO) so it does not re-fire, then raise
    // mip.SSIP so the delegated S-mode soft interrupt runs the remote
    // sfence_vma_all in srv::trap.
    csrr t1, mhartid
    li t2, 0x02000000
    add t2, t2, t1
    sw zero, 0(t2)
    li t2, 0x2
    csrs mip, t2
    j .Lm32_restore
.Lm32_restore:
    lw a1, 12(t0)
    lw a0, 8(t0)
    lw t2, 4(t0)
    lw t1, 0(t0)
    csrrw t0, mscratch, t0
    mret
"#
);

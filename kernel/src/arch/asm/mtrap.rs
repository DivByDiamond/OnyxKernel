//! M-mode trap handler for the non-`smode` (self-hosted QEMU) boot chain.
//!
//! Root cause (see OnyxKernel/todo.md, `usleep()`/`nanosleep()` hang,
//! 2026-09-02): this kernel's own M→S boot transition (`boot.rs`) zeroes
//! `mie` and `mret`s into S-mode once, never returning to M-mode. The CLINT
//! `mtimecmp` MMIO comparator only ever asserts `mip.MTIP`, an M-mode-only
//! interrupt that RISC-V's `mideleg` cannot delegate to S-mode — so without
//! something in M-mode to forward it, MTIP sits pending forever and no
//! S-mode `wfi` waiting on the timer can ever wake.
//!
//! This is the "something": a minimal M-mode trap vector implementing the
//! legacy SBI v0.1 `SBI_SET_TIMER` contract that `arch::sbi::set_timer`
//! already speaks (previously only exercised by the `smode`/OC2R build
//! against real OpenSBI). It handles exactly two causes, both expected —
//! everything else `boot.rs`'s `medeleg` already routes straight to S-mode:
//!
//!   - `ecall` from S-mode (cause 9, un-delegated in `medeleg` for this
//!     reason): `a0` is the target absolute `mtime` value. Writes the
//!     calling hart's CLINT `mtimecmp` slot, clears `mip.STIP` (ack — a
//!     previous tick's forwarded interrupt, if any, is now superseded by
//!     the fresh deadline) and sets `mie.MTIE` (re-arms *this* mechanism:
//!     see the MTI case below for why it was cleared), then returns.
//!   - Machine timer interrupt (cause 7, async, never delegable): sets
//!     `mip.STIP` (forwards it to S-mode, where `mideleg` bit 5 delegates
//!     STI and `sie.STIE` is already enabled) and clears `mie.MTIE` (the
//!     CLINT comparator condition — `mtime >= mtimecmp` — stays true until
//!     something rearms it with a fresh, later value; leaving MTIE set
//!     would re-trap on every instruction in the meantime).
//!
//! No general-purpose register is touched without being saved: `mscratch`
//! is set up per-hart (in `boot.rs` / `arch/smp/secondary.rs`) to point at
//! this hart's row of `G_MTRAP_SCRATCH`, mirroring the `sscratch` pattern
//! `trap_asm.rs` already uses for S-mode traps — safe even though an MTI
//! can interrupt arbitrary S-mode code with no stack we're allowed to use.
use core::arch::global_asm;

/// Per-hart save area for `t1`/`t2`/`a0` — the only registers `mtrap_entry`
/// touches. Indexed by `mhartid`; `mscratch` on each hart points directly
/// at its own row (set once per hart at M-mode boot, before that hart's
/// first possible M-mode trap).
#[unsafe(no_mangle)]
pub static mut G_MTRAP_SCRATCH: [[u64; 3]; crate::arch::smp::MAX_HARTS] =
    [[0; 3]; crate::arch::smp::MAX_HARTS];

global_asm!(
    r#"
.section .text.boot
.balign 4
.global mtrap_entry
mtrap_entry:
    csrrw t0, mscratch, t0
    sd t1, 0(t0)
    sd t2, 8(t0)
    sd a0, 16(t0)
    csrr t1, mcause
    bltz t1, .Lm_interrupt
    // Exception: only ecall-from-S-mode (medeleg leaves every other
    // exception delegated straight to S-mode) should ever reach here.
    li t2, 9
    bne t1, t2, .Lm_restore
    csrr t2, mhartid
    slli t2, t2, 3
    li t1, 0x02004000
    add t1, t1, t2
    li t2, 0xFFFFFFFF
    sw t2, 4(t1)
    sw a0, 0(t1)
    srli t2, a0, 32
    sw t2, 4(t1)
    li t2, 0x20
    csrc mip, t2
    li t2, 0x80
    csrs mie, t2
    csrr t2, mepc
    addi t2, t2, 4
    csrw mepc, t2
    j .Lm_restore
.Lm_interrupt:
    andi t2, t1, 0xf
    li t1, 7
    bne t2, t1, .Lm_restore
    li t2, 0x80
    csrc mie, t2
    li t2, 0x20
    csrs mip, t2
.Lm_restore:
    ld a0, 16(t0)
    ld t2, 8(t0)
    ld t1, 0(t0)
    csrrw t0, mscratch, t0
    mret
"#
);

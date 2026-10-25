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
//! This is the "something": a minimal M-mode trap vector that stands in for
//! SBI firmware (there is none under `-bios none` / OnyxBoot) and implements
//! the three ecall extensions the kernel speaks:
//!
//!   - **Base (EID 0x10)** — `get_spec_version` (FID 0), `get_impl_id` (1),
//!     `get_impl_version` (2) and `probe_extension` (3). We advertise SBI
//!     v2.0 so callers can feature-probe before using SRST; `probe_extension`
//!     answers 1 for exactly the extensions this shim services and 0 for
//!     everything else (a real probe, not a hardcoded "yes").
//!   - **SRST (EID "SRST")** — `system_reset` (FID 0). There is no firmware
//!     below us to reset the SoC, so the shim drives the QEMU-virt
//!     `sifive_test` finisher directly: 0x5555 powers off, 0x7777 resets.
//!     Any other reset type is treated as a reset.
//!   - **Legacy v0.1 `SBI_SET_TIMER` (EID 0)** — absolute `mtime` in `a0`
//!     (split into `a0`/`a1` on rv32). Writes the calling hart's CLINT
//!     `mtimecmp`, clears `mip.STIP` (ack — a previous tick is superseded
//!     by the fresh deadline) and sets `mie.MTIE` (re-arms *this* mechanism:
//!     see the MTI case below for why it was cleared), then returns.
//!
//! Unknown EIDs get `SBI_ERR_NOT_SUPPORTED` (-2) in `a0` instead of being
//! silently misread as a timer arm — the previous behaviour made any future
//! SBI call look like it succeeded while reprogramming the clock.
//!
//! Interrupt causes handled (everything else `boot.rs`'s `medeleg` already
//! routes straight to S-mode):
//!
//!   - Machine timer interrupt (cause 7, async, never delegable): sets
//!     `mip.STIP` (forwards it to S-mode, where `mideleg` bit 5 delegates
//!     STI and `sie.STIE` is already enabled) and clears `mie.MTIE` (the
//!     CLINT comparator condition — `mtime >= mtimecmp` — stays true until
//!     something rearms it with a fresh, later value; leaving MTIE set
//!     would re-trap on every instruction in the meantime).
//!   - Machine software interrupt (cause 3): cross-hart IPI doorbell, see
//!     `.Lm_soft_fwd`.
//!
//! No general-purpose register is touched without being saved: `mscratch`
//! is set up per-hart (in `boot.rs` / `arch/smp/secondary.rs`) to point at
//! this hart's row of `G_MTRAP_SCRATCH`, mirroring the `sscratch` pattern
//! `trap_asm.rs` already uses for S-mode traps — safe even though an MTI
//! can interrupt arbitrary S-mode code with no stack we're allowed to use.
//!
//! Register protocol with `arch::sbi`: the ecall wrapper declares `a0`/`a1`
//! as clobbered and reads them back, so the shim must leave results there.
//! That is why the restore path reloads `a0`/`a1` from the scratch row —
//! each handler stores its outputs into those slots before returning.
use core::arch::global_asm;

/// Per-hart save area, indexed by `mhartid` (`mscratch` on each hart points
/// at its own row, set once per hart at M-mode boot before that hart's first
/// possible M-mode trap). Layout: `t1`, `t2`, `a0`, `a1`, `a7` — `a0`/`a1`
/// double as the ecall output slots (see the module comment), `a7` carries
/// the SBI extension id.
#[unsafe(no_mangle)]
pub static mut G_MTRAP_SCRATCH: [[u64; 5]; crate::arch::smp::MAX_HARTS] =
    [[0; 5]; crate::arch::smp::MAX_HARTS];

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
    sd a1, 24(t0)
    sd a7, 32(t0)
    csrr t1, mcause
    bltz t1, .Lm_interrupt
    // Exception: only ecall-from-S-mode (medeleg leaves every other
    // exception delegated straight to S-mode) should ever reach here.
    li t2, 9
    bne t1, t2, .Lm_restore
    csrr t2, mepc
    addi t2, t2, 4
    csrw mepc, t2
    // Dispatch on the SBI extension id in a7.
    li t2, 0
    beq a7, t2, .Lm_set_timer
    li t2, 0x10
    beq a7, t2, .Lm_base
    li t2, 0x53525354
    beq a7, t2, .Lm_srst
    // Unknown extension: a0 = SBI_ERR_NOT_SUPPORTED (-2), a1 = 0.
    li t2, -2
    sd t2, 16(t0)
    sd zero, 24(t0)
    j .Lm_restore
.Lm_set_timer:
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
    j .Lm_restore
.Lm_base:
    // a6 = function id.
    li t2, 0
    beq a6, t2, .Lm_base_version
    li t2, 1
    beq a6, t2, .Lm_base_impl_id
    li t2, 2
    beq a6, t2, .Lm_base_impl_version
    li t2, 3
    beq a6, t2, .Lm_base_probe
    j .Lm_not_supported
.Lm_base_version:
    sd zero, 16(t0)
    li t1, 0x02000000
    sd t1, 24(t0)
    j .Lm_restore
.Lm_base_impl_id:
    sd zero, 16(t0)
    li t1, 0x4F4E5958
    sd t1, 24(t0)
    j .Lm_restore
.Lm_base_impl_version:
    sd zero, 16(t0)
    li t1, 1
    sd t1, 24(t0)
    j .Lm_restore
.Lm_base_probe:
    // a0 = extension id to probe: true for BASE and SRST, false otherwise.
    li t1, 0x10
    beq a0, t1, .Lm_probe_yes
    li t1, 0x53525354
    beq a0, t1, .Lm_probe_yes
    sd zero, 16(t0)
    sd zero, 24(t0)
    j .Lm_restore
.Lm_probe_yes:
    sd zero, 16(t0)
    li t1, 1
    sd t1, 24(t0)
    j .Lm_restore
.Lm_not_supported:
    li t1, -2
    sd t1, 16(t0)
    sd zero, 24(t0)
    j .Lm_restore
.Lm_srst:
    // Only FID 0 (system_reset) exists; a0 = reset type, a1 = reason.
    li t1, 0
    bne a6, t1, .Lm_not_supported
    sd zero, 16(t0)
    sd zero, 24(t0)
    // No firmware below us: drive the QEMU-virt sifive_test finisher.
    li t1, 0x100000
    li t2, 0x5555
    beqz a0, 1f
    li t2, 0x7777
1:  sw t2, 0(t1)
    j .Lm_restore
.Lm_interrupt:
    andi t2, t1, 0xf
    li t1, 7
    beq t2, t1, .Lm_timer_fwd
    li t1, 3
    beq t2, t1, .Lm_soft_fwd
    j .Lm_restore
.Lm_timer_fwd:
    li t2, 0x80
    csrc mie, t2
    li t2, 0x20
    csrs mip, t2
    j .Lm_restore
.Lm_soft_fwd:
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
    j .Lm_restore
.Lm_restore:
    ld a1, 24(t0)
    ld a0, 16(t0)
    ld t2, 8(t0)
    ld t1, 0(t0)
    csrrw t0, mscratch, t0
    mret
"#
);

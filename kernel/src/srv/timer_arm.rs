//! Low-level timer-comparator plumbing for `srv::timer`, split out to keep
//! `timer.rs` under the project's 250-line file limit.
//!
//! `read_mtime` and `arm_timer` are the only two operations that differ
//! between the `smode` (OC2R/OpenSBI) build and the self-hosted QEMU
//! build (non-`smode`: rv64 via `arch::asm::mtrap`, rv32 via
//! `arch::asm::mtrap_32` as of 2026-09-12) — see `arm_timer`'s doc comment
//! for the full story on why the CLINT `mtimecmp` MMIO register alone can
//! never wake an S-mode `wfi`, and OnyxKernel/todo.md's 2026-09-02 (rv64)
//! and 2026-09-12 (rv32) entries for how that was root-caused. The raw
//! CLINT-MMIO path below (`write_mtimecmp`/`arm_timer_for_hart`) now only
//! exists as a `cfg(test)` stub for host unit tests, which have no real
//! CSRs/firmware to ecall into.
#[cfg(not(feature = "smode"))]
use crate::arch::mmio::Mmio;
#[cfg(not(feature = "smode"))]
use crate::arch::regs::CLINT_BASE;

#[cfg(not(feature = "smode"))]
pub(super) static mut G_MTIME: usize = 0;
#[cfg(not(feature = "smode"))]
pub(super) static mut G_MTIMECMP: usize = 0;

/// # Safety
///
/// Boot-time only, before any `read_mtime`/`arm_timer` call: sets the
/// CLINT MMIO base addresses used by the non-`smode` builds.
pub(super) unsafe fn init_mmio_bases() {
    #[cfg(not(feature = "smode"))]
    {
        // SAFETY: single boot-hart call, before any reader; not(smode)-only statics.
        unsafe {
            G_MTIME = (CLINT_BASE + 0xBFF8) as usize;
            G_MTIMECMP = (CLINT_BASE + 0x4000) as usize;
        }
    }
}

#[cfg(feature = "smode")]
/// # Safety
///
/// S-mode only: reads the `time` CSR, valid whenever S-mode timer access
/// is permitted (mcounteren/firmware).
pub(super) unsafe fn read_mtime() -> u64 {
    // SAFETY: pure read of the read-only `time` CSR; no memory access or side effects.
    unsafe {
        // sedna uses an ACLINT timer (mtime at 0x02004FF8, not the legacy CLINT
        // 0x0200BFF8), so the MMIO offset is wrong there. The `time` CSR (0xC01)
        // always reflects the current timer value and is readable from S-mode.
        crate::arch::csr::read_time()
    }
}

#[cfg(not(feature = "smode"))]
/// # Safety
///
/// M-mode-boot kernels only: `G_MTIME` must have been set by
/// `init_mmio_bases` to the CLINT mtime address; the hi/lo re-read loop
/// tolerates a torn 64-bit read across the two 32-bit MMIO halves.
pub(super) unsafe fn read_mtime() -> u64 {
    // SAFETY: MMIO reads at the init-established CLINT mtime base; the hi==hi2 retry restores read consistency.
    unsafe {
        loop {
            let hi = Mmio::<u32>::at(G_MTIME + 4).read();
            let lo = Mmio::<u32>::at(G_MTIME).read();
            let hi2 = Mmio::<u32>::at(G_MTIME + 4).read();
            if hi == hi2 {
                return ((hi as u64) << 32) | (lo as u64);
            }
        }
    }
}

#[cfg(any(feature = "smode", not(test)))]
/// # Safety
///
/// S-mode: issues the SBI SetTimer ecall, which arms THIS hart's timer
/// delegate; `next` is an absolute mtime value. On the `smode` (OC2R)
/// build a real OpenSBI firmware services this. On non-`smode` builds
/// (self-hosted QEMU), a minimal M-mode trap vector services it instead —
/// `arch/asm/mtrap.rs` on rv64, `arch/asm/mtrap_32.rs` on rv32 (ported
/// 2026-09-12, todo.md "rv32 mtrap": same MTIP->STIP forwarding fix as
/// rv64, just splitting the 64-bit `stime` across a0/a1 since rv32
/// registers are 32 bits wide — see `arch::sbi::set_timer`'s rv32
/// variant). See either mtrap file's doc comment for why writing the
/// CLINT `mtimecmp` MMIO register directly (still used by the cfg(test)
/// stub below, host-only) can never wake an S-mode `wfi` on this boot
/// chain (mip.MTIP is never forwarded to S-mode's STIP without it).
pub(super) unsafe fn arm_timer(next: u64) {
    // SAFETY: ecall issued from S-mode; serviced by OpenSBI (smode) or
    // this kernel's own mtrap_entry/mtrap_entry_32 (non-smode), both of
    // which arm the calling hart's timer by the legacy SBI_SET_TIMER
    // contract.
    unsafe {
        crate::arch::sbi::set_timer(next);
    }
}

#[cfg(all(not(feature = "smode"), test))]
/// # Safety
///
/// Host unit tests only: writes hart 0's mtimecmp via `write_mtimecmp`
/// instead of issuing a real ecall (there is no CSR/firmware to ecall
/// into on the host target). Every real (non-test) build — rv64 and rv32
/// alike — goes through `arm_timer`'s SBI_SET_TIMER path above.
pub(super) unsafe fn arm_timer(next: u64) {
    // SAFETY: MMIO write to hart 0's comparator, test-only per the contract above.
    unsafe {
        write_mtimecmp(next);
    }
}

#[cfg(all(not(feature = "smode"), test))]
/// # Safety
///
/// Targets G_MTIMECMP (hart 0's slot, set by `init_mmio_bases`); the
/// 0xFFFFFFFF guard-write prevents a spurious interrupt between the two
/// half-writes of the 64-bit comparator.
unsafe fn write_mtimecmp(v: u64) {
    // SAFETY: ordered volatile MMIO writes at the init-established address; the guard value avoids spurious ticks mid-update.
    unsafe {
        Mmio::<u32>::at(G_MTIMECMP + 4).write(0xFFFF_FFFF);
        Mmio::<u32>::at(G_MTIMECMP).write(v as u32);
        Mmio::<u32>::at(G_MTIMECMP + 4).write((v >> 32) as u32);
    }
}

/// # Safety
///
/// Host unit tests only: writes `hartid`'s CLINT mtimecmp slot directly.
/// Every real (non-test) build now goes through `arm_timer`'s SBI ecall
/// (inherently per-hart — see its doc comment) instead.
#[cfg(all(not(feature = "smode"), test))]
pub(super) unsafe fn arm_timer_for_hart(hartid: usize, next: u64) {
    // SAFETY: per-hart comparator address; ordered guarded MMIO writes as in write_mtimecmp.
    unsafe {
        let cmp_addr = crate::arch::regs::clint_mtimecmp_hart(hartid) as usize;
        Mmio::<u32>::at(cmp_addr + 4).write(0xFFFF_FFFF);
        Mmio::<u32>::at(cmp_addr).write(next as u32);
        Mmio::<u32>::at(cmp_addr + 4).write((next >> 32) as u32);
    }
}

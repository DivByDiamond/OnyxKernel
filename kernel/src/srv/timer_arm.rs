//! Low-level timer-comparator plumbing for `srv::timer`, split out to keep
//! `timer.rs` under the project's 250-line file limit.
//!
//! `read_mtime` and `arm_timer` are the only two operations that differ
//! between the `smode` (OC2R/OpenSBI) build and the self-hosted QEMU
//! build (rv64 non-`smode` via `arch::asm::mtrap`, rv32 non-`smode` via
//! raw CLINT MMIO) — see `arm_timer`'s doc comment for the full story on
//! why the CLINT `mtimecmp` MMIO register alone can never wake an S-mode
//! `wfi`, and OnyxKernel/todo.md's 2026-09-02 entry for how that was
//! root-caused.
#[cfg(not(feature = "smode"))]
use crate::arch::mmio::Mmio;
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
    // SAFETY: single boot-hart call, before any reader; not(smode)-only statics.
    unsafe {
        #[cfg(not(feature = "smode"))]
        {
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

#[cfg(any(feature = "smode", all(not(test), target_pointer_width = "64")))]
/// # Safety
///
/// S-mode: issues the SBI SetTimer ecall, which arms THIS hart's timer
/// delegate; `next` is an absolute mtime value. On the `smode` (OC2R)
/// build a real OpenSBI firmware services this. On the rv64 non-`smode`
/// (self-hosted QEMU) build, `arch/asm/mtrap.rs`'s `mtrap_entry` — this
/// kernel's own minimal M-mode trap vector — services it instead: see its
/// doc comment for why writing the CLINT `mtimecmp` MMIO register directly
/// (the old rv64 behavior, still used on rv32 below) can never wake an
/// S-mode `wfi` on this boot chain (mip.MTIP is never forwarded to
/// S-mode's STIP without it).
pub(super) unsafe fn arm_timer(next: u64) {
    // SAFETY: ecall issued from S-mode; serviced by OpenSBI (smode) or
    // this kernel's own mtrap_entry (rv64 non-smode), both of which arm
    // the calling hart's timer by the legacy SBI_SET_TIMER contract.
    unsafe {
        crate::arch::sbi::set_timer(next);
    }
}

#[cfg(all(not(feature = "smode"), any(test, target_pointer_width = "32")))]
/// # Safety
///
/// M-mode-boot rv32 kernels only: writes hart 0's mtimecmp via
/// `write_mtimecmp`; called from `srv::timer::init()` before secondary
/// harts exist.
///
/// rv32 has no `mtrap.rs` yet (see `arch/asm/mod.rs`) — this still writes
/// the CLINT MMIO comparator directly, which only asserts `mip.MTIP` and
/// can never wake an S-mode `wfi` on this boot chain (mie is zeroed at
/// boot and never returns to M-mode) — same root cause as the rv64
/// non-`smode` build had before `mtrap_entry` was added, just not yet
/// fixed for rv32.
pub(super) unsafe fn arm_timer(next: u64) {
    // SAFETY: MMIO write to hart 0's comparator, boot-time-only per the contract above.
    unsafe {
        write_mtimecmp(next);
    }
}

#[cfg(all(not(feature = "smode"), any(test, target_pointer_width = "32")))]
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
/// Per-hart, `not(smode)` rv32 only: writes `hartid`'s CLINT mtimecmp
/// slot directly. Called from `srv::timer::init_hart`/`handle` on the
/// rv32 build (the only one still lacking a per-hart M-mode forwarder).
#[cfg(all(not(feature = "smode"), any(test, target_pointer_width = "32")))]
pub(super) unsafe fn arm_timer_for_hart(hartid: usize, next: u64) {
    // SAFETY: per-hart comparator address; ordered guarded MMIO writes as in write_mtimecmp.
    unsafe {
        let cmp_addr = crate::arch::regs::clint_mtimecmp_hart(hartid) as usize;
        Mmio::<u32>::at(cmp_addr + 4).write(0xFFFF_FFFF);
        Mmio::<u32>::at(cmp_addr).write(next as u32);
        Mmio::<u32>::at(cmp_addr + 4).write((next >> 32) as u32);
    }
}

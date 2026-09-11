//! Minimal legacy SBI v0.1 wrappers for S-mode (OC2R/OpenSBI) operation.
//!
//! Everything in this module is only valid when the kernel actually runs in
//! S-mode under an SBI firmware (OpenSBI / OC2R ROM). Booted via OnyxBoot the
//! hart is handed over in M-mode, and an `ecall` from M-mode has nowhere to
//! go: it traps to `mtvec`, which nobody set, i.e. address 0 — a silent
//! machine death with no panic and no serial output. `hart_in_m_mode()`
//! exists so the boot path can detect that mismatch and fail loudly instead.

/// True if the current hart is executing in M-mode.
///
/// Detection probe: reading `mcounteren` is legal in M-mode and raises an
/// illegal-instruction exception in S-mode. We install a one-shot local trap
/// handler around the read; if the exception fires we know we are NOT in
/// M-mode. This is the standard Linux-style privilege sniff; it requires the
/// firmware to delegate illegal-instruction exceptions to S-mode, which both
/// OpenSBI and OC2R do.
///
/// The handler must work no matter which privilege level it itself executes
/// at, so it branches on `sstatus.SPP` of the trapped context: previous
/// privilege S means the fault was delegated and is recorded in `sepc`
/// (return via `sret`); previous M would mean it landed in `mepc` (return
/// via `mret`). In practice only the delegated-S case can occur: when this
/// code already runs in M-mode the probe CSR read simply succeeds.
#[cfg(feature = "smode")]
pub fn hart_in_m_mode() -> bool {
    let out: usize;
    // SAFETY: the asm touches only declared-output registers and CSRs
    // (stvec is saved and restored; no memory is read or written).
    unsafe {
        core::arch::asm!(
            ".option push",
            ".option norvc",
            "csrr  t2, stvec",        // preserve the live stvec
            "la    t0, 3f",
            "csrw  stvec, t0",
            "li    t3, 1",            // presume M-mode
            "csrr  t1, mcounteren",   // illegal in S-mode -> traps to 3:
            "j     4f",
            ".p2align 2",
            "3:",                     // one-shot handler
            "li    t3, 0",            // trap fired => not M-mode
            "csrr  t0, sepc",
            "addi  t0, t0, 4",        // skip the faulting csrr
            "csrw  sepc, t0",
            "csrr  t0, sstatus",
            "srli  t0, t0, 8",
            "andi  t0, t0, 1",        // sstatus.SPP of the trapped context
            "bnez  t0, 5f",           // came from S -> delegated -> sret
            "mret",                   // defensive M-return path
            "5:   sret",
            "4:",
            "csrw  stvec, t2",        // restore stvec
            ".option pop",
            out("t3") out,
            out("t0") _,
            out("t1") _,
            out("t2") _,
        );
    }
    out != 0
}

/// Arm the S-mode timer. Legacy SBI_SET_TIMER: ecall with a7=0, a0=absolute stime value.
/// OpenSBI services the underlying MTIP and delivers us an STIP.
/// # Safety
///
/// Must be called in S-mode under an SBI firmware that implements the
/// legacy v0.1 SBI_SET_TIMER extension; an `ecall` with no SBI beneath
/// (e.g. from M-mode booted via OnyxBoot) traps to an unset mtvec.
pub unsafe fn set_timer(stime: u64) {
    // SAFETY: ecall with a7=0 / a0=stime is the legacy SBI_SET_TIMER contract.
    //
    // Root cause (KDF/hash_password nondeterminism under long-running user
    // loops, 2026-09-05): this asm! block only declared `a0`/`a7` as inputs.
    // The SBI ecall ABI returns its status/value in a0/a1 — the firmware
    // (OpenSBI, or this kernel's own mtrap_entry servicing the same legacy
    // SBI_SET_TIMER contract) WRITES both on return. Because `a1` was never
    // declared to the compiler at all (not even as clobbered), LLVM was free
    // to keep some other live value cached in `a1` across this inline `ecall`
    // — after inlining `set_timer` into `arm_timer`/`timer::handle`, whatever
    // happened to be live in `a1` at that point (once every ~10ms, on every
    // timer tick) got silently overwritten by the firmware's real return
    // value. This reproduced identically under both the smode/OpenSBI and
    // non-smode/mtrap_entry builds (both route through this same function)
    // and was independent of the scheduler, paging, and QEMU itself —
    // confirmed by a bare-metal, kernel-only repro with interrupts enabled.
    // Declaring both registers `lateout` (clobbered, value discarded) tells
    // the compiler this ecall may destroy them, matching the real ABI.
    unsafe {
        core::arch::asm!(
            "ecall",
            in("a7") 0usize,
            inlateout("a0") stime as usize => _,
            lateout("a1") _,
            options(nostack),
        );
    }
}

// ---------------------------------------------------------------------------
// SBI v2.0-style calls (base + SRST).
//
// In non-`smode` builds there is no real firmware beneath the kernel: the
// only ecall the M-mode shim (`arch/asm/mtrap.rs`) services is the legacy
// SBI_SET_TIMER (a7=0) — any other EID would be misinterpreted as a timer
// arm. So these entry points are no-ops there and callers must fall back
// (see `klog::reset_machine`'s QEMU finisher path).
// ---------------------------------------------------------------------------

/// Legacy "SBI" probe extension EID (FID 0: sbi_probe_extension).
const SBI_EID_PROBE: usize = 0x0053_4249;
/// Base extension EID (FID 0: sbi_get_spec_version).
const SBI_EID_BASE: usize = 0x0000_0010;
/// System Reset extension EID ("SRST"), FID 0: sbi_system_reset.
const SBI_EID_SRST: usize = 0x5352_5354;

/// SRST reset types (SBI v2.0, FID 0).
pub const SRST_SHUTDOWN: u64 = 0;
pub const SRST_COLD_REBOOT: u64 = 1;
pub const SRST_WARM_REBOOT: u64 = 2;

/// Raw SBI v0.2+ ecall: a7 = EID, a6 = FID; returns (error, value).
///
/// # Safety
///
/// Must run in S-mode with an SBI beneath that implements the extension
/// being called; from M-mode (or the mtrap shim) only a7=0 is defined.
#[inline]
unsafe fn sbi_ecall(eid: usize, fid: usize, a0: usize, a1: usize) -> (isize, usize) {
    let err: usize;
    let val: usize;
    // SAFETY: ecall per the SBI v0.2 ABI; a0/a1 both carry the return
    // values on exit (same clobber contract as set_timer above).
    unsafe {
        core::arch::asm!(
            "ecall",
            in("a7") eid,
            in("a6") fid,
            inlateout("a0") a0 => err,
            inlateout("a1") a1 => val,
            options(nostack),
        );
    }
    (err as isize, val)
}

/// Firmware SBI spec version (base extension, FID 0), major<<24 | minor.
/// `None` when there is no SBI (non-`smode` builds — issuing the ecall
/// would reach the mtrap shim, which only understands a7=0).
#[cfg(feature = "smode")]
pub fn get_spec_version() -> Option<u64> {
    // SAFETY: S-mode ecall under real firmware (feature-gated, see above).
    let (err, val) = unsafe { sbi_ecall(SBI_EID_BASE, 0, 0, 0) };
    (err == 0).then_some(val as u64)
}

#[cfg(not(feature = "smode"))]
pub fn get_spec_version() -> Option<u64> {
    None
}

/// SBI System Reset (SRST, FID 0): `reset_type` 0 = shutdown, 1 = cold
/// reboot, 2 = warm reboot. Returns `false` when SRST is unavailable
/// (no SBI, probe negative, or the ecall reported an error) so the caller
/// can use a platform fallback. On success the machine resets and this
/// never returns.
pub fn system_reset(reset_type: u64) -> bool {
    #[cfg(feature = "smode")]
    {
        // SAFETY: S-mode ecall under real firmware (feature-gated above).
        let (err, _) = unsafe {
            let (err, _) = sbi_ecall(SBI_EID_PROBE, 0, SBI_EID_SRST, 0);
            if err != 0 {
                return false;
            }
            sbi_ecall(SBI_EID_SRST, 0, reset_type as usize, 1) // behavior: IMMEDIATE
        };
        err == 0
    }
    #[cfg(not(feature = "smode"))]
    {
        let _ = reset_type;
        false
    }
}

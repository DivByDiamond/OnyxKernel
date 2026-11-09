//! SBI wrappers shared by both boot chains.
//!
//! * S-mode under real firmware (`--features smode`, OpenSBI / OC2R): these
//!   are ordinary SBI ecalls.
//! * Booted via OnyxBoot (`-bios none`, no firmware at all): the hart starts
//!   in M-mode and the kernel's own M-mode trap shim (`arch/asm/mtrap.rs`,
//!   rv32: `mtrap_32.rs`) services the exact same ecall contract. Without
//!   that shim an ecall from S-mode would trap to an unset `mtvec`, i.e.
//!   address 0 — a silent machine death. `hart_in_m_mode()` exists so the
//!   boot path can detect the mismatch (S-mode kernel, M-mode handoff) and
//!   fail loudly instead.
//!
//! Extensions implemented on both paths: legacy v0.1 `SBI_SET_TIMER`, SBI
//! v0.2 base (`get_spec_version`, `get_impl_id`, `get_impl_version`,
//! `probe_extension`) and SRST v2.0 `system_reset` (see the tail of this
//! file for what the shim does with a reset request).

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

/// Arm the S-mode timer. Legacy SBI_SET_TIMER: ecall with a7=0, absolute
/// stime value. On rv64 the whole 64-bit value fits in a0; the legacy v0.1
/// ABI on rv32 splits it across a0 (low 32 bits) and a1 (high 32 bits) —
/// see the rv32 variant below. OpenSBI services the underlying MTIP and
/// delivers us an STIP (this kernel's own `mtrap`/`mtrap_32` do the same
/// when booted without a real firmware beneath it).
/// # Safety
///
/// Must be called in S-mode under an SBI firmware that implements the
/// legacy v0.1 SBI_SET_TIMER extension; an `ecall` with no SBI beneath
/// (e.g. from M-mode booted via OnyxBoot) traps to an unset mtvec.
#[cfg(target_pointer_width = "64")]
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

/// rv32 variant (todo.md "rv32 mtrap", 2026-09-12): registers are 32 bits
/// wide, so the 64-bit `stime` value cannot fit in a0 alone like the rv64
/// path above — the legacy v0.1 ABI defines it as two REAL inputs, a0 (low
/// 32 bits) and a1 (high 32 bits), not just two clobbered outputs. Both
/// this wrapper and the M-mode handler that services it without real
/// firmware (`arch::asm::mtrap_32`) must agree on this split.
#[cfg(target_pointer_width = "32")]
pub unsafe fn set_timer(stime: u64) {
    // SAFETY: ecall with a7=0 / a0=low32(stime) / a1=high32(stime) is the
    // legacy SBI_SET_TIMER contract on rv32; both registers are declared
    // `inlateout` (real input, clobbered on return) per the same audited
    // reasoning as the rv64 variant above.
    unsafe {
        core::arch::asm!(
            "ecall",
            in("a7") 0usize,
            inlateout("a0") stime as usize => _,
            inlateout("a1") (stime >> 32) as usize => _,
            options(nostack),
        );
    }
}

// ---------------------------------------------------------------------------
// SBI v0.2+ base extension and SRST.
//
// Both boot chains answer the same probe, so callers do not need to know
// which one they run on: OpenSBI answers for real, and the M-mode shim
// (`arch/asm/mtrap*.rs`) advertises v2.0 with `probe_extension` returning
// true for exactly the extensions it services.
// ---------------------------------------------------------------------------

/// Legacy "SBI" probe extension EID (FID 0: sbi_probe_extension). Only used
/// as a fallback for firmware predating the v0.2 base extension.
const SBI_EID_PROBE: usize = 0x0053_4249;
/// SBI v0.2 base extension EID.
const SBI_EID_BASE: usize = 0x0000_0010;
/// SBI v2.0 System Reset extension EID ("SRST"), FID 0: system_reset.
const SBI_EID_SRST: usize = 0x5352_5354;

/// Base-extension function ids.
const BASE_GET_SPEC_VERSION: usize = 0;
const BASE_GET_IMPL_ID: usize = 1;
const BASE_GET_IMPL_VERSION: usize = 2;
const BASE_PROBE_EXTENSION: usize = 3;

/// `SBI_SUCCESS` from the v0.2 error-code space.
const SBI_SUCCESS: isize = 0;

mod reset;

pub use reset::{SRST_COLD_REBOOT, SRST_SHUTDOWN, SRST_WARM_REBOOT, system_reset};

/// Raw SBI v0.2+ ecall: a7 = EID, a6 = FID; returns (error, value).
///
/// # Safety
///
/// Must run in S-mode with an SBI implementation beneath that services the
/// extension being called: real firmware, or this kernel's M-mode shim when
/// booted via OnyxBoot.
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
/// `None` when the base extension is missing or the call failed.
pub fn get_spec_version() -> Option<u64> {
    let (err, val) = unsafe { sbi_ecall(SBI_EID_BASE, BASE_GET_SPEC_VERSION, 0, 0) };
    (err == SBI_SUCCESS).then_some(val as u64)
}

/// Decoded `(major, minor)` from `get_spec_version`.
pub fn spec_version_parts() -> Option<(u32, u32)> {
    let v = get_spec_version()?;
    Some((((v >> 24) & 0x7F) as u32, (v & 0x00FF_FFFF) as u32))
}

/// Firmware implementation id (base extension, FID 1).
pub fn get_impl_id() -> Option<u64> {
    let (err, val) = unsafe { sbi_ecall(SBI_EID_BASE, BASE_GET_IMPL_ID, 0, 0) };
    (err == SBI_SUCCESS).then_some(val as u64)
}

/// Firmware implementation version (base extension, FID 2).
pub fn get_impl_version() -> Option<u64> {
    let (err, val) = unsafe { sbi_ecall(SBI_EID_BASE, BASE_GET_IMPL_VERSION, 0, 0) };
    (err == SBI_SUCCESS).then_some(val as u64)
}

/// True when the layer beneath implements the extension `eid`.
///
/// Uses the v0.2 base-extension probe, falling back to the legacy "SBI"
/// probe extension for older firmware, so this is a real capability check
/// rather than a hardcoded "yes".
pub fn probe_extension(eid: u64) -> bool {
    let (err, val) = unsafe { sbi_ecall(SBI_EID_BASE, BASE_PROBE_EXTENSION, eid as usize, 0) };
    if err == SBI_SUCCESS {
        return val != 0;
    }
    let (err, val) = unsafe { sbi_ecall(SBI_EID_PROBE, 0, eid as usize, 0) };
    err == SBI_SUCCESS && val != 0
}

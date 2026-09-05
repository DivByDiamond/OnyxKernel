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

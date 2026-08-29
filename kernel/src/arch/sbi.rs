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
    unsafe {
        core::arch::asm!(
            "ecall",
            in("a7") 0usize,
            in("a0") stime as usize,
            options(nostack),
        );
    }
}

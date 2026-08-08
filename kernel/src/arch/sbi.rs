//! Minimal legacy SBI v0.1 wrappers for S-mode (OC2R/OpenSBI) operation.

/// Arm the S-mode timer. Legacy SBI_SET_TIMER: ecall with a7=0, a0=absolute stime value.
/// OpenSBI services the underlying MTIP and delivers us an STIP.
pub unsafe fn set_timer(stime: u64) {
    core::arch::asm!(
        "ecall",
        in("a7") 0usize,
        in("a0") stime as usize,
        options(nostack),
    );
}

//! SBI System Reset (SRST v2.0) — the reset half of the SBI wrappers.

/// SRST reset types (SBI v2.0, FID 0).
pub const SRST_SHUTDOWN: u64 = 0;
pub const SRST_COLD_REBOOT: u64 = 1;
pub const SRST_WARM_REBOOT: u64 = 2;
/// SRST reset reason: "some unexpected failure" (the SBI default).
const SRST_REASON_SYSTEM_FAILURE: u64 = 1;

/// SBI System Reset (SRST, FID 0): `reset_type` 0 = shutdown, 1 = cold
/// reboot, 2 = warm reboot. Returns `false` when SRST is unavailable (no
/// implementation, probe negative, or the ecall reported an error) so the
/// caller can use a platform fallback. On success the machine resets and
/// this never returns.
pub fn system_reset(reset_type: u64) -> bool {
    if !super::probe_extension(super::SBI_EID_SRST as u64) {
        return false;
    }
    let (err, _) = unsafe {
        super::sbi_ecall(
            super::SBI_EID_SRST,
            0,
            reset_type as usize,
            SRST_REASON_SYSTEM_FAILURE as usize,
        )
    };
    err == super::SBI_SUCCESS
}

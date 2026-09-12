//! Interrupt-cause handling (timer, external/PLIC, cross-hart soft IPI).
use crate::arch::regs::*;
use crate::drivers::plic;
use crate::srv::timer;

/// Dispatch an interrupt-class `scause` code (the `SCAUSE_INT` bit already
/// stripped by the caller) to its handler.
///
/// # Safety
///
/// Must be called from the top-level trap dispatcher (`super::handle`) with
/// SIE cleared, on the hart that just trapped; relies on the same contract
/// as `super::handle`.
pub unsafe fn handle(code: u64) {
    // SAFETY: called only from `super::handle`, which upholds the
    // S-mode / SIE-cleared / live-trap-context contract documented above.
    unsafe {
        match code {
            INTR_S_TIMER => timer::handle(),
            INTR_S_EXTERN => {
                plic::dispatch();
            }
            INTR_S_SOFT => {
                // Cross-hart TLB-shootdown IPI (destroy_root broadcast,
                // forwarded from M-mode by mtrap_entry): ack the pending
                // S-soft bit and flush this hart's TLB. sfence.vma is
                // hart-local, so a remote address-space teardown needs
                // this per-hart flush or stale TLB entries keep freed
                // page-table/data pages writable from this hart.
                crate::arch::csr::clear_sip(0x2);
                crate::arch::csr::sfence_vma_all();
            }
            _ => {
                crate::kwrn!(
                    "trap",
                    "unhandled interrupt: code=%d",
                    onyx_core::fmt::Arg::from(code)
                );
            }
        }
    }
}

//! Power management — idle/sleep states + WFI dispatcher.
//!
//! Provides a thin abstraction over RISC-V S-mode power management:
//!   - `idle()` issues WFI until the next interrupt.
//!   - `sleep()` disables the timer and enters a long WFI loop.
//!   - `wake()` re-arms the timer tick.
//!
//! Full DVFS / domain power-down is delegated to `cpufreq` and to the
//! board's PMIC (not yet modelled here).
use crate::arch::csr;

static mut G_DEEP_SLEEP: bool = false;

/// Enter the lightest idle state: a single WFI. The caller is expected
/// to call this in a tight loop in the idle thread.
pub fn idle() {
    // SAFETY: csr::wfi is a trivially valid privileged WFI per the RISC-V privileged spec; returns on the next interrupt.
    unsafe {
        csr::wfi();
    }
}

/// Enter deep sleep: disable timer interrupts, mask SIE, then WFI until
/// any external interrupt arrives. Use `wake()` to resume the tick.
///
/// # Safety
/// Disables the timer tick; only safe to call from the idle hart when
/// no other harts rely on the timer for preemption.
pub unsafe fn sleep() {
    // SAFETY: G_DEEP_SLEEP flag write and SIE CSR manipulation are hart-local privileged state per the RISC-V privileged spec; no shared memory race (SIE=0 during the loop).
    unsafe {
        G_DEEP_SLEEP = true;
        csr::clear_sie(crate::arch::regs::SSTATUS_SIE);
        // Loop until an external interrupt fires.
        loop {
            csr::wfi();
            if !G_DEEP_SLEEP {
                break;
            }
        }
    }
}

/// Resume from deep sleep: re-enable SIE and the timer tick.
///
/// # Safety
///
/// Caller contract: must run on the hart that called `sleep()`; re-enables
/// SIE and re-inits the timer, so it must not race a concurrent tick.
pub unsafe fn wake() {
    // SAFETY: SIE CSR set is a hart-local privileged operation per the RISC-V privileged spec; G_DEEP_SLEEP write is single-threaded resume context.
    unsafe {
        G_DEEP_SLEEP = false;
        csr::set_sie(crate::arch::regs::SSTATUS_SIE);
        crate::srv::timer::init_hart(0);
    }
}

/// Returns true if the system is currently in deep sleep.
pub fn is_sleeping() -> bool {
    // SAFETY: G_DEEP_SLEEP is a plain kernel-owned flag; read without concurrency (kernel never runs with SIE set).
    unsafe { G_DEEP_SLEEP }
}

/// Request a graceful shutdown through the syscon device.
pub fn shutdown() -> ! {
    crate::drivers::syscon::poweroff()
}

/// Request a reboot through the syscon device.
pub fn reboot() -> ! {
    crate::drivers::syscon::reboot()
}

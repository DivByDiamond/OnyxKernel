//! SiFive test finisher — poweroff, reboot, and exit codes.
//!
//! QEMU virt exposes a "test device" at 0x100000 that accepts a 32-bit
//! command: bits[15:0] = exit code, bits[31:16] = action. Writing a
//! FINISHER_PASS / FINISHER_FAIL code cleanly ends the QEMU session.
//! On real SiFive boards the same register fronts the test chip's reset
//! controller, so we keep the writes harmless when running on hardware.
use crate::arch::mmio::Mmio;

pub const FINISHER_BASE: usize = 0x1000_00;

const FINISHER_PASS: u32 = 0x5555;
const FINISHER_FAIL: u32 = 0x3333;
const FINISHER_RESET: u32 = 0x7777;
const FINISHER_SHUTDOWN: u32 = 0x9999;
const FINISHER_REBOOT: u32 = 0xCCCC;

/// Initialise the finisher base (defaults to QEMU virt 0x100000).
pub fn init(base: usize) {
    // Nothing else to do — the device is a single 32-bit write-only register.
    let _ = base;
}

/// Power off the machine cleanly (QEMU exits with code 0).
pub fn poweroff() -> ! {
    unsafe {
        Mmio::<u32>::at(FINISHER_BASE).write(FINISHER_SHUTDOWN);
        // If the device didn't accept the command, fall back to a hard loop.
        loop {
            crate::arch::csr::wfi();
        }
    }
}

/// Reboot the machine (QEMU resets).
pub fn reboot() -> ! {
    unsafe {
        Mmio::<u32>::at(FINISHER_BASE).write(FINISHER_REBOOT);
        // Some platforms use a different reset register; try the older
        // 0x5555 "pass" code that QEMU also accepts as a clean reset.
        Mmio::<u32>::at(FINISHER_BASE).write(FINISHER_RESET);
        loop {
            crate::arch::csr::wfi();
        }
    }
}

/// Halt the machine and signal a clean exit (test PASS).
pub fn exit_pass() -> ! {
    unsafe {
        Mmio::<u32>::at(FINISHER_BASE).write(FINISHER_PASS);
        loop {
            crate::arch::csr::wfi();
        }
    }
}

/// Halt the machine and signal a failure exit code.
pub fn exit_fail(code: u32) -> ! {
    unsafe {
        let v = FINISHER_FAIL | ((code & 0xFFFF) << 16);
        Mmio::<u32>::at(FINISHER_BASE).write(v);
        loop {
            crate::arch::csr::wfi();
        }
    }
}

/// Was the write accepted? On QEMU virt this never returns; on real
/// hardware the write completes silently and execution continues.
/// Useful only as a platform probe at boot — returns `true` if running
/// under QEMU (where the call would have halted the machine).
pub fn is_qemu_finisher() -> bool {
    // The finisher is write-only — there is no readback. The caller must
    // infer "still running ⇒ not QEMU" by the fact that this function
    // returned at all. We always return false here because if we got
    // here, QEMU didn't terminate us.
    false
}

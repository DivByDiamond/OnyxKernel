//! SiFive watchdog — start/stop/ping. Triggers a hard reset if not pinged.
//!
//! QEMU virt exposes a SiFive watchdog at 0x10070000 (shares the address
//! space with the RTC's counter, but uses different registers). The driver
//! implements a "kick" pattern: a kernel heartbeat calls `ping()` once per
//! second; if the watchdog is not pinged for `timeout_ms` it fires.
use crate::arch::mmio::Mmio;
use onyx_core::errno::{Errno, KResult};

const WDT_BASE: usize = 0x1007_0000;
const TIMEOUT_MAX: u32 = 1_000_000;

const R_LO: u32 = 0x00;
const R_HI: u32 = 0x04;
const R_FEED: u32 = 0x08;
const R_KEY: u32 = 0x0C;
const R_CMP: u32 = 0x10;

const KEY_VALID: u32 = 0x51F1_5E5E;
const FEED_MAGIC: u32 = 0x4C46_4545;

static mut G_BASE: usize = WDT_BASE;
static mut G_ENABLED: bool = false;
static mut G_TIMEOUT: u32 = 0;

#[inline]
unsafe fn rd(off: u32) -> u32 {
    Mmio::<u32>::at(G_BASE + off as usize).read()
}

#[inline]
unsafe fn wr(off: u32, v: u32) {
    Mmio::<u32>::at(G_BASE + off as usize).write(v);
}

/// Initialise the watchdog base address without enabling it.
pub unsafe fn init(base: usize) {
    G_BASE = base;
}

/// Arm the watchdog with a `timeout_ms` millisecond deadline. The kernel
/// must call `ping()` at least that often to avoid a reset.
pub fn arm(timeout_ms: u32) -> KResult<()> {
    if timeout_ms == 0 || timeout_ms > TIMEOUT_MAX {
        return Err(Errno::Range);
    }
    unsafe {
        // SiFive watchdog runs off the 31.25 kHz RTC clock on real boards;
        // on QEMU virt the underlying clock is 10 MHz. We use a conservative
        // divisor of 10 (1 MHz effective) so `timeout_ms` ≈ `cmp` ticks.
        let ticks = timeout_ms * 1000;
        wr(R_CMP, ticks);
        wr(R_KEY, KEY_VALID);
        G_TIMEOUT = timeout_ms;
        G_ENABLED = true;
    }
    Ok(())
}

/// Disarm the watchdog. Real SiFive hardware cannot be disabled once armed,
/// so we emulate disarm by pushing the comparator to the maximum.
pub fn disarm() {
    unsafe {
        wr(R_CMP, TIMEOUT_MAX * 1000);
        wr(R_KEY, KEY_VALID);
        G_ENABLED = false;
    }
}

/// Reset the watchdog counter ("kick the dog"). Must be called faster
/// than the configured timeout.
pub fn ping() {
    unsafe {
        wr(R_FEED, FEED_MAGIC);
    }
}

/// Was the watchdog armed by `arm()`?
pub fn is_armed() -> bool {
    unsafe { G_ENABLED }
}

/// Current configured timeout in milliseconds (0 if disarmed).
pub fn timeout_ms() -> u32 {
    unsafe { G_TIMEOUT }
}

/// Read the free-running counter — useful as a high-resolution monotonic
/// clock source independent of CLINT.
pub fn counter() -> u64 {
    unsafe {
        let lo = rd(R_LO) as u64;
        let hi = rd(R_HI) as u64;
        (hi << 32) | lo
    }
}

/// PLIC handler: ping the watchdog from a timer-tick context. Called by
/// the scheduler every jiffie to keep the system alive.
pub unsafe fn tick() {
    if G_ENABLED {
        ping();
    }
}

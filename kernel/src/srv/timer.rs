//! CLINT timer (100 Hz tick).
//!
//! Tick counters use the widest atomically-accessible integer on the
//! target: AtomicU64 on rv64, AtomicU32 on rv32 (riscv32imac has no
//! 64-bit AMOs). At 100 Hz a u32 jiffies counter wraps after ~497 days;
//! all readers use wrapping arithmetic (`uptime_us`, `sys_nanosleep`),
//! so wraparound degrades gracefully instead of panicking.
//!
//! Low-level comparator plumbing (`read_mtime`/`arm_timer`, and why the
//! rv64 non-`smode` build routes arming through `arch::asm::mtrap` rather
//! than the CLINT MMIO register directly) lives in `timer_arm` — split out
//! to keep this file under the project's 250-line limit.
use super::timer_arm;

#[cfg(target_pointer_width = "32")]
use core::sync::atomic::AtomicU32 as AtomicTick;
#[cfg(target_pointer_width = "64")]
use core::sync::atomic::AtomicU64 as AtomicTick;
use core::sync::atomic::Ordering;

use crate::{arch::csr, proc};
#[cfg(all(not(feature = "smode"), any(test, target_pointer_width = "32")))]
use timer_arm::arm_timer_for_hart;
use timer_arm::{arm_timer, read_mtime};

static mut G_FREQ: u64 = crate::arch::regs::CLINT_FREQ_QEMU;
static mut G_TICK_INTERVAL: u64 = 0;
// Audit fix: G_UPTICKS/G_JIFFIES were `static mut u64` with non-atomic RMW
// from every ticking hart — concurrent increments could be lost. Both are now
// only written via atomic fetch_add.
static G_UPTICKS: AtomicTick = AtomicTick::new(0);
/// Monotonic tick counter, incremented once per timer tick per hart.
/// Written ONLY through the atomic view below (`AtomicTick::from_ptr`);
/// external readers must go through [`jiffies`], which performs an
/// atomic Acquire load and widens to u64.
#[cfg(target_pointer_width = "64")]
pub static mut G_JIFFIES: u64 = 0;
#[cfg(target_pointer_width = "32")]
pub static mut G_JIFFIES: u32 = 0;

#[inline]
fn jiffies_atomic() -> &'static AtomicTick {
    // SAFETY: G_JIFFIES is a naturally-aligned machine-word static of the
    // matching atomic type that outlives the program; the pointer stays
    // valid forever. Same pattern as arch/smp.rs G_RELEASE.
    unsafe { AtomicTick::from_ptr(core::ptr::addr_of_mut!(G_JIFFIES)) }
}

/// Atomic snapshot of the monotonic tick counter, widened to u64.
/// On rv32 the underlying u32 wraps after ~497 days at 100 Hz; callers
/// already treat the value as wrapping.
#[inline]
#[must_use]
pub fn jiffies() -> u64 {
    #[cfg(target_pointer_width = "64")]
    {
        jiffies_atomic().load(Ordering::Acquire)
    }
    #[cfg(target_pointer_width = "32")]
    {
        jiffies_atomic().load(Ordering::Acquire) as u64
    }
}

/// # Safety
///
/// Boot-time: must run once on the boot hart before secondary harts arm
/// their own comparators (init_hart); sets the CLINT statics and arms
/// hart 0's comparator.
pub unsafe fn init() {
    // SAFETY: single boot-hart call; MMIO bases (if any) are written before any reader runs.
    unsafe {
        timer_arm::init_mmio_bases();
        G_FREQ = crate::arch::regs::CLINT_FREQ_QEMU;
        G_TICK_INTERVAL = G_FREQ / 100;
        let now = read_mtime();
        arm_timer(now + G_TICK_INTERVAL);
        csr::set_sie(1 << 5);
        crate::kinf!(
            "timer",
            "CLINT @%p, tick=%d ns",
            onyx_core::fmt::Arg::from(crate::arch::regs::CLINT_BASE),
            onyx_core::fmt::Arg::from(1_000_000_000u64 / G_FREQ)
        );
    }
}

/// # Safety
///
/// Per-hart: must run once per hart at startup with its own `hartid`;
/// writes that hart's mtimecmp slot and enables the S-mode timer
/// interrupt locally. No shared state is mutated.
pub unsafe fn init_hart(hartid: usize) {
    // SAFETY: per-hart comparator address derived from the passed hartid; one call per hart at startup.
    unsafe {
        let now = read_mtime();
        let next = now + G_TICK_INTERVAL;
        #[cfg(any(feature = "smode", all(not(test), target_pointer_width = "64")))]
        {
            // arm_timer's SBI ecall is inherently per-hart (each hart's
            // ecall is serviced independently), so no address computation
            // from hartid is needed here — unlike the rv32 CLINT-MMIO path.
            let _ = hartid;
            arm_timer(next);
        }
        #[cfg(all(not(feature = "smode"), any(test, target_pointer_width = "32")))]
        arm_timer_for_hart(hartid, next);
        csr::set_sie(1 << 5);
    }
}

/// # Safety
///
/// Timer-interrupt context: runs on the trapped hart with SIE cleared
/// (same-hart preemption impossible; other harts may tick concurrently).
/// Tick counters are atomic fetch_add, and the comparator write targets
/// this hart only.
pub unsafe fn handle() {
    // SAFETY: G_UPTICKS/G_JIFFIES are atomic fetch_add so cross-hart ticks are never lost; the comparator write targets this hart only.
    unsafe {
        // Atomic RMW so ticks from different harts are never lost.
        G_UPTICKS.fetch_add(1, Ordering::Relaxed);
        jiffies_atomic().fetch_add(1, Ordering::Relaxed);
        let now = read_mtime();
        let next = now + G_TICK_INTERVAL;
        // Re-arm the tick for THIS hart via arm_timer's SBI ecall (OpenSBI
        // on `smode`, this kernel's own `mtrap_entry` on rv64 non-`smode`).
        // rv32 non-`smode` still writes the per-hart CLINT mtimecmp MMIO
        // directly (never hart 0's — that bug caused a timer storm on
        // harts 1..N) — see timer_arm::arm_timer's doc comments.
        #[cfg(any(feature = "smode", all(not(test), target_pointer_width = "64")))]
        arm_timer(next);
        #[cfg(all(not(feature = "smode"), any(test, target_pointer_width = "32")))]
        arm_timer_for_hart(crate::arch::smp::current_hart(), next);
        proc::sched_tick();
        // Heartbeat: ping the watchdog every tick (100 Hz) so the system
        // resets if a scheduler stall prevents us from reaching this point.
        crate::drivers::watchdog::tick();
        // LED activity pulse once every 50 ticks (~0.5 s at 100 Hz).
        if G_UPTICKS.load(Ordering::Relaxed).is_multiple_of(50) {
            crate::drivers::led::pulse_activity();
        }
        // Kernel event loop (todo P3 #3): pump input devices and run due
        // soft timers once per tick so TUI event loops stay live while
        // user processes sleep in poll().
        crate::srv::event::pump();
    }
}
pub fn uptime_us() -> u64 {
    // Acquire load: readers of uptime get a consistent (if wrapping) sample.
    // The multiply wraps on rv64 like the old code; on rv32 the widened
    // product cannot overflow for any u32 tick count.
    #[cfg(target_pointer_width = "64")]
    {
        G_UPTICKS.load(Ordering::Acquire).wrapping_mul(10_000)
    }
    #[cfg(target_pointer_width = "32")]
    {
        (G_UPTICKS.load(Ordering::Acquire) as u64).wrapping_mul(10_000)
    }
}

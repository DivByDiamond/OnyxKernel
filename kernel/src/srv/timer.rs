//! CLINT timer (100 Hz tick).
//!
//! Tick counters use the widest atomically-accessible integer on the
//! target: AtomicU64 on rv64, AtomicU32 on rv32 (riscv32imac has no
//! 64-bit AMOs). At 100 Hz a u32 jiffies counter wraps after ~497 days;
//! all readers use wrapping arithmetic (`uptime_us`, `sys_nanosleep`),
//! so wraparound degrades gracefully instead of panicking.
#[cfg(target_pointer_width = "32")]
use core::sync::atomic::AtomicU32 as AtomicTick;
#[cfg(target_pointer_width = "64")]
use core::sync::atomic::AtomicU64 as AtomicTick;
use core::sync::atomic::Ordering;

#[cfg(not(feature = "smode"))]
use crate::arch::mmio::Mmio;
use crate::{
    arch::{csr, regs::*},
    proc,
};
#[cfg(not(feature = "smode"))]
static mut G_MTIME: usize = 0;
#[cfg(not(feature = "smode"))]
static mut G_MTIMECMP: usize = 0;
static mut G_FREQ: u64 = CLINT_FREQ_QEMU;
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
    // SAFETY: single boot-hart call; G_MTIME/G_MTIMECMP are written here before any read_mtime/write_mtimecmp use them.
    unsafe {
        let clint = CLINT_BASE;
        #[cfg(not(feature = "smode"))]
        {
            G_MTIME = (clint + 0xBFF8) as usize;
            G_MTIMECMP = (clint + 0x4000) as usize;
        }
        G_FREQ = CLINT_FREQ_QEMU;
        G_TICK_INTERVAL = G_FREQ / 100;
        let now = read_mtime();
        arm_timer(now + G_TICK_INTERVAL);
        csr::set_sie(1 << 5);
        crate::kinf!(
            "timer",
            "CLINT @%p, tick=%d ns",
            onyx_core::fmt::Arg::from(clint),
            onyx_core::fmt::Arg::from(1_000_000_000u64 / G_FREQ)
        );
    }
}

#[cfg(feature = "smode")]
/// # Safety
///
/// S-mode only: reads the `time` CSR, valid whenever S-mode timer access
/// is permitted (mcounteren/firmware).
unsafe fn read_mtime() -> u64 {
    // SAFETY: pure read of the read-only `time` CSR; no memory access or side effects.
    unsafe {
        // sedna uses an ACLINT timer (mtime at 0x02004FF8, not the legacy CLINT
        // 0x0200BFF8), so the MMIO offset is wrong there. The `time` CSR (0xC01)
        // always reflects the current timer value and is readable from S-mode.
        crate::arch::csr::read_time()
    }
}

#[cfg(not(feature = "smode"))]
/// # Safety
///
/// M-mode-boot kernels only: `G_MTIME` must have been set by init() to
/// the CLINT mtime address; the hi/lo re-read loop tolerates a torn
/// 64-bit read across the two 32-bit MMIO halves.
unsafe fn read_mtime() -> u64 {
    // SAFETY: MMIO reads at the init-established CLINT mtime base; the hi==hi2 retry restores read consistency.
    unsafe {
        loop {
            let hi = Mmio::<u32>::at(G_MTIME + 4).read();
            let lo = Mmio::<u32>::at(G_MTIME).read();
            let hi2 = Mmio::<u32>::at(G_MTIME + 4).read();
            if hi == hi2 {
                return ((hi as u64) << 32) | (lo as u64);
            }
        }
    }
}

#[cfg(feature = "smode")]
/// # Safety
///
/// S-mode only: issues the SBI SetTimer ecall, which arms THIS hart's
/// timer delegate; `next` is an absolute mtime value.
unsafe fn arm_timer(next: u64) {
    // SAFETY: SBI ecall issued from S-mode; the SBI arms the calling hart's timer by spec.
    unsafe {
        crate::arch::sbi::set_timer(next);
    }
}

#[cfg(not(feature = "smode"))]
/// # Safety
///
/// M-mode-boot kernels only: writes hart 0's mtimecmp via
/// write_mtimecmp; called from init() before secondary harts exist.
unsafe fn arm_timer(next: u64) {
    // SAFETY: MMIO write to hart 0's comparator, boot-time-only per the contract above.
    unsafe {
        write_mtimecmp(next);
    }
}

#[cfg(not(feature = "smode"))]
/// # Safety
///
/// Targets G_MTIMECMP (hart 0's slot, set by init()); the 0xFFFFFFFF
/// guard-write prevents a spurious interrupt between the two half-writes
/// of the 64-bit comparator.
unsafe fn write_mtimecmp(v: u64) {
    // SAFETY: ordered volatile MMIO writes at the init-established address; the guard value avoids spurious ticks mid-update.
    unsafe {
        Mmio::<u32>::at(G_MTIMECMP + 4).write(0xFFFF_FFFF);
        Mmio::<u32>::at(G_MTIMECMP).write(v as u32);
        Mmio::<u32>::at(G_MTIMECMP + 4).write((v >> 32) as u32);
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
        #[cfg(feature = "smode")]
        {
            let _ = hartid;
            arm_timer(next);
        }
        #[cfg(not(feature = "smode"))]
        {
            let cmp_addr = clint_mtimecmp_hart(hartid) as usize;
            Mmio::<u32>::at(cmp_addr + 4).write(0xFFFF_FFFF);
            Mmio::<u32>::at(cmp_addr).write(next as u32);
            Mmio::<u32>::at(cmp_addr + 4).write((next >> 32) as u32);
        }
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
        // Re-arm the tick for THIS hart. In M-mode we write the per-hart
        // mtimecmp directly (never hart 0's — that bug caused a timer storm
        // on harts 1..N). In S-mode (OC2R) we arm via the SBI timer, which
        // OpenSBI forwards to us as an STIP.
        #[cfg(feature = "smode")]
        arm_timer(next);
        #[cfg(not(feature = "smode"))]
        {
            let hartid = crate::arch::smp::current_hart();
            let cmp_addr = clint_mtimecmp_hart(hartid) as usize;
            Mmio::<u32>::at(cmp_addr + 4).write(0xFFFF_FFFF);
            Mmio::<u32>::at(cmp_addr).write(next as u32);
            Mmio::<u32>::at(cmp_addr + 4).write((next >> 32) as u32);
        }
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

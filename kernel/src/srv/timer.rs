//! CLINT timer (100 Hz tick).
use crate::arch::regs::*;
use crate::arch::{csr, mmio::Mmio};
use crate::proc;
#[cfg(not(feature = "smode"))]
static mut G_MTIME: usize = 0;
#[cfg(not(feature = "smode"))]
static mut G_MTIMECMP: usize = 0;
static mut G_FREQ: u64 = CLINT_FREQ_QEMU;
static mut G_TICK_INTERVAL: u64 = 0;
static mut G_UPTICKS: u64 = 0;
pub static mut G_JIFFIES: u64 = 0;

pub unsafe fn init() {
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

#[cfg(feature = "smode")]
unsafe fn read_mtime() -> u64 {
    // sedna uses an ACLINT timer (mtime at 0x02004FF8, not the legacy CLINT
    // 0x0200BFF8), so the MMIO offset is wrong there. The `time` CSR (0xC01)
    // always reflects the current timer value and is readable from S-mode.
    crate::arch::csr::read_time()
}

#[cfg(not(feature = "smode"))]
unsafe fn read_mtime() -> u64 {
    loop {
        let hi = Mmio::<u32>::at(G_MTIME + 4).read();
        let lo = Mmio::<u32>::at(G_MTIME).read();
        let hi2 = Mmio::<u32>::at(G_MTIME + 4).read();
        if hi == hi2 {
            return ((hi as u64) << 32) | (lo as u64);
        }
    }
}

#[cfg(feature = "smode")]
unsafe fn arm_timer(next: u64) {
    crate::arch::sbi::set_timer(next);
}

#[cfg(not(feature = "smode"))]
unsafe fn arm_timer(next: u64) {
    write_mtimecmp(next);
}

#[cfg(not(feature = "smode"))]
unsafe fn write_mtimecmp(v: u64) {
    Mmio::<u32>::at(G_MTIMECMP + 4).write(0xFFFF_FFFF);
    Mmio::<u32>::at(G_MTIMECMP).write(v as u32);
    Mmio::<u32>::at(G_MTIMECMP + 4).write((v >> 32) as u32);
}

pub unsafe fn init_hart(hartid: usize) {
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

pub unsafe fn handle() {
    G_UPTICKS = G_UPTICKS.wrapping_add(1);
    G_JIFFIES = G_JIFFIES.wrapping_add(1);
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
    if G_UPTICKS % 50 == 0 {
        crate::drivers::led::pulse_activity();
    }
}
pub fn uptime_us() -> u64 {
    unsafe { G_UPTICKS * 10_000 }
}

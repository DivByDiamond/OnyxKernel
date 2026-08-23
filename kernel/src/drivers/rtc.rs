//! SiFive-style RTC — wall-clock time source for FS timestamps.
//!
//! QEMU virt exposes a Goldfish-compatible RTC at 0x101000; the SiFive FU540
//! RTC lives at 0x10070000 on real boards. The driver probes both addresses
//! and exposes a single `now_secs()` API used by onyxfs for ctime/mtime.
use crate::arch::mmio::Mmio;
use onyx_core::errno::{Errno, KResult};

const GOLDFISH_RTC_BASE: usize = 0x1010_0000;
const SIFIVE_RTC_BASE: usize = 0x1007_0000;

// Goldfish RTC registers (QEMU virt)
const GF_TIME_LOW: u32 = 0x00;
const GF_TIME_HIGH: u32 = 0x04;
const GF_ALARM_LOW: u32 = 0x08;
const GF_ALARM_HIGH: u32 = 0x0C;
const GF_IRQ_ENABLE: u32 = 0x10;
const GF_CLEAR_ALARM: u32 = 0x14;

// SiFive RTC registers (real boards)
const S5_RTC_LO: u32 = 0x00;
const S5_RTC_HI: u32 = 0x04;
const _S5_RTC_CFG: u32 = 0x40;

static mut G_BASE: usize = 0;
static mut G_KIND: RtcKind = RtcKind::None;

#[derive(Clone, Copy, PartialEq)]
enum RtcKind {
    None,
    Goldfish,
    SiFive,
}

#[inline]
unsafe fn rd32(base: usize, off: u32) -> u32 { unsafe {
    Mmio::<u32>::at(base + off as usize).read()
}}

#[inline]
unsafe fn wr32(base: usize, off: u32, v: u32) { unsafe {
    Mmio::<u32>::at(base + off as usize).write(v);
}}

/// Probe for a known RTC. `fdt_base` is the base from the device tree (0 if
/// none). When the FDT provides a base we probe ONLY that address — on
/// OC2R/sedna the RTC lives at the FDT address and the hardcoded QEMU
/// goldfish base (0x10100000) is not a real device there (load access fault).
pub unsafe fn probe(fdt_base: usize) -> bool { unsafe {
    if fdt_base != 0 {
        return probe_at(fdt_base, RtcKind::Goldfish);
    }
    if probe_at(GOLDFISH_RTC_BASE, RtcKind::Goldfish) {
        return true;
    }
    probe_at(SIFIVE_RTC_BASE, RtcKind::SiFive)
}}

unsafe fn probe_at(base: usize, kind: RtcKind) -> bool { unsafe {
    let t1 = read_kind(base, kind);
    if t1 == 0 {
        return false;
    }
    // Spin briefly — clock must advance.
    for _ in 0..1000 {
        if read_kind(base, kind) != t1 {
            G_BASE = base;
            G_KIND = kind;
            return true;
        }
    }
    false
}}

unsafe fn read_kind(base: usize, kind: RtcKind) -> u64 { unsafe {
    match kind {
        RtcKind::Goldfish => {
            let lo = rd32(base, GF_TIME_LOW) as u64;
            let hi = rd32(base, GF_TIME_HIGH) as u64;
            (hi << 32) | lo
        }
        RtcKind::SiFive => {
            let lo = rd32(base, S5_RTC_LO) as u64;
            let hi = rd32(base, S5_RTC_HI) as u64;
            (hi << 32) | lo
        }
        RtcKind::None => 0,
    }
}}

/// Wall-clock seconds since Unix epoch. Returns 0 if no RTC was probed.
pub fn now_secs() -> u64 {
    unsafe {
        match G_KIND {
            RtcKind::Goldfish => read_kind(G_BASE, RtcKind::Goldfish),
            RtcKind::SiFive => read_kind(G_BASE, RtcKind::SiFive) / (10_000_000),
            RtcKind::None => 0,
        }
    }
}

/// Wall-clock nanoseconds since Unix epoch.
pub fn now_nanos() -> u64 {
    unsafe { read_kind(G_BASE, G_KIND) }
}

/// Program a one-shot alarm `secs` seconds in the future and enable IRQ.
/// `irq` is the PLIC IRQ the RTC is wired to (QEMU virt: IRQ 11).
pub unsafe fn arm_alarm(secs: u64, irq: u32) -> KResult<()> { unsafe {
    if G_KIND != RtcKind::Goldfish {
        return Err(Errno::NoSys);
    }
    let now = now_nanos();
    // Goldfish alarm is in nanoseconds.
    let target = now + secs * 1_000_000_000;
    wr32(G_BASE, GF_ALARM_HIGH, (target >> 32) as u32);
    wr32(G_BASE, GF_ALARM_LOW, target as u32);
    wr32(G_BASE, GF_IRQ_ENABLE, 1);
    crate::drivers::plic::set_priority(irq, 4);
    crate::drivers::plic::enable(irq, 0);
    Ok(())
}}

/// Clear a pending alarm interrupt.
pub unsafe fn clear_alarm() { unsafe {
    if G_KIND == RtcKind::Goldfish {
        wr32(G_BASE, GF_CLEAR_ALARM, 1);
    }
}}

/// Base address of the active RTC (0 if none).
pub fn base() -> usize {
    unsafe { G_BASE }
}

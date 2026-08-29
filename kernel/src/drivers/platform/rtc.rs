//! SiFive-style RTC — wall-clock time source for entropy mixing.
//!
//! QEMU virt exposes a Goldfish-compatible RTC at 0x101000; the SiFive FU540
//! RTC lives at 0x10070000 on real boards. The driver probes both addresses
//! and exposes a single `now_nanos()` API (used by hwrand for entropy mixing;
//! OnyxFS timestamps use timer::jiffies instead).
use crate::arch::mmio::Mmio;

const GOLDFISH_RTC_BASE: usize = 0x1010_0000;
const SIFIVE_RTC_BASE: usize = 0x1007_0000;

// Goldfish RTC registers (QEMU virt)
const GF_TIME_LOW: u32 = 0x00;
const GF_TIME_HIGH: u32 = 0x04;

// SiFive RTC registers (real boards)
const S5_RTC_LO: u32 = 0x00;
const S5_RTC_HI: u32 = 0x04;

static mut G_BASE: usize = 0;
static mut G_KIND: RtcKind = RtcKind::None;

#[derive(Clone, Copy, PartialEq)]
enum RtcKind {
    None,
    Goldfish,
    SiFive,
}

#[inline]
/// # Safety
///
/// Caller contract: `base` must be an RTC MMIO base (probed/FDT-provided)
/// and `off` an RTC register offset for that device.
unsafe fn rd32(base: usize, off: u32) -> u32 {
    // SAFETY: `base` is either an FDT-probed RTC base or one of the fixed SoC constants below; `off` is a datasheet register offset within the RTC MMIO window.
    unsafe { Mmio::<u32>::at(base + off as usize).read() }
}

/// Probe for a known RTC. `fdt_base` is the base from the device tree (0 if
/// none). When the FDT provides a base we probe ONLY that address — on
/// OC2R/sedna the RTC lives at the FDT address and the hardcoded QEMU
/// goldfish base (0x10100000) is not a real device there (load access fault).
pub unsafe fn probe(fdt_base: usize) -> bool {
    // SAFETY: only known RTC bases are dereferenced (FDT base when provided, else the fixed GOLDFISH/SIFIVE constants); writes G_BASE/G_KIND single-threaded.
    unsafe {
        if fdt_base != 0 {
            return probe_at(fdt_base, RtcKind::Goldfish);
        }
        if probe_at(GOLDFISH_RTC_BASE, RtcKind::Goldfish) {
            return true;
        }
        probe_at(SIFIVE_RTC_BASE, RtcKind::SiFive)
    }
}

/// # Safety
///
/// Caller contract: `base` must be a real RTC MMIO base matching `kind`
/// (within the device MMIO window); runs single-threaded before harts
/// publish the global base.
unsafe fn probe_at(base: usize, kind: RtcKind) -> bool {
    // SAFETY: read_kind reads only `base` + datasheet offsets; G_BASE/G_KIND writes happen in single-threaded probe context.
    unsafe {
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
    }
}

/// # Safety
///
/// Caller contract: `base` must be a real RTC MMIO base matching `kind`
/// (within the device MMIO window).
unsafe fn read_kind(base: usize, kind: RtcKind) -> u64 {
    // SAFETY: rd32 reads `base` + fixed datasheet offsets (0x00/0x04) for the given RTC kind; `base` lies in the device MMIO window.
    unsafe {
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
    }
}

/// Wall-clock nanoseconds since Unix epoch (0 if no RTC was probed).
pub fn now_nanos() -> u64 {
    // SAFETY: G_BASE/G_KIND were set by probe() to a validated RTC base; read_kind reads only fixed datasheet offsets there.
    unsafe { read_kind(G_BASE, G_KIND) }
}

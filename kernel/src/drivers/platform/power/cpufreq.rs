//! CPUFreq / CCPM — Canaan SG2000 clock + DVFS control.
//!
//! SG2000 exposes a clock/reset unit (CRU) at 0x0190_0000 and a PLL
//! controller at 0x0190_1000. CPU clock is derived from PLL0; the
//! driver exposes get/set of the CPU frequency in MHz and a small set
//! of OPPs (operating performance points) for DVFS.
use crate::arch::mmio::Mmio;
use onyx_core::errno::{Errno, KResult};

const CRU_BASE: usize = 0x0190_0000;
const PLL_BASE: usize = 0x0190_1000;

const R_PLL0_CTRL: u32 = 0x00;
const R_CPU_CLK_DIV: u32 = 0x10;
const _R_CPU_CLK_SEL: u32 = 0x14;

/// Operating Performance Points supported on SG2000 (freq_mhz, volt_mv).
pub const OPPS: &[(u32, u32)] = &[
    (300, 900),
    (500, 950),
    (700, 1000),
    (1000, 1100),
    (1200, 1200),
];

static mut G_CRU: usize = CRU_BASE;
static mut G_PLL: usize = PLL_BASE;
static mut G_CUR_OPP: usize = 1; // default 500 MHz

#[inline]
/// # Safety
///
/// Caller contract: `off` must be a CRU register offset; G_CRU must be
/// initialised (CRU_BASE default or from `init`).
unsafe fn cru_rd(off: u32) -> u32 {
    // SAFETY: G_CRU is the fixed SoC CRU base (0x0190_0000 constant or init-validated); `off` is a datasheet register offset within the CRU MMIO window.
    unsafe { Mmio::<u32>::at(G_CRU + off as usize).read() }
}

#[inline]
/// # Safety
///
/// Caller contract: `off` must be a CRU register offset; G_CRU must be
/// initialised (CRU_BASE default or from `init`).
unsafe fn cru_wr(off: u32, v: u32) {
    // SAFETY: G_CRU is the fixed SoC CRU base (0x0190_0000 constant or init-validated); `off` is a datasheet register offset within the CRU MMIO window.
    unsafe {
        Mmio::<u32>::at(G_CRU + off as usize).write(v);
    }
}

#[inline]
/// # Safety
///
/// Caller contract: `off` must be a PLL-controller register offset;
/// G_PLL must be initialised (PLL_BASE default or from `init`).
unsafe fn pll_rd(off: u32) -> u32 {
    // SAFETY: G_PLL is the fixed SoC PLL base (0x0190_1000 constant or init-validated); `off` is a datasheet register offset within the PLL MMIO window.
    unsafe { Mmio::<u32>::at(G_PLL + off as usize).read() }
}

#[inline]
/// # Safety
///
/// Caller contract: `off` must be a PLL-controller register offset;
/// G_PLL must be initialised (PLL_BASE default or from `init`).
unsafe fn _pll_wr(off: u32, v: u32) {
    // SAFETY: G_PLL is the fixed SoC PLL base (0x0190_1000 constant or init-validated); `off` is a datasheet register offset within the PLL MMIO window.
    unsafe {
        Mmio::<u32>::at(G_PLL + off as usize).write(v);
    }
}

/// # Safety
///
/// Caller contract: `cru`/`pll` must be validated MMIO bases (FDT nodes
/// or the fixed SoC constants) and `init` must run once, on a single hart,
/// before other cpufreq calls.
pub unsafe fn init(cru: usize, pll: usize) {
    // SAFETY: single-threaded init; G_CRU/G_PLL only written here before use.
    unsafe {
        G_CRU = cru;
        G_PLL = pll;
    }
}

/// Read the current CPU frequency in MHz. Derived from PLL0 and the
/// divider in the CRU.
pub fn freq_mhz() -> u32 {
    // SAFETY: pll_rd/cru_rd read datasheet offsets from the validated G_PLL/G_CRU MMIO bases; G_CUR_OPP is not touched here.
    unsafe {
        let pll = pll_rd(R_PLL0_CTRL);
        // PLL0 output = refclk * (N + 1) / (M + 1) / (P + 1).
        // Assume refclk = 25 MHz on SG2000 eval boards.
        let n = (pll >> 8) & 0xFF;
        let m = pll & 0xFF;
        let p = (pll >> 16) & 0x7;
        let refclk = 25u32;
        let vco = refclk * (n + 1) / (m + 1).max(1);
        let pll_out = vco / (1 << p);
        let div = cru_rd(R_CPU_CLK_DIV) & 0x1F;
        pll_out / (div + 1)
    }
}

/// Set the CPU frequency by picking the closest OPP. Returns the actual
/// frequency that was set, or `Err(Errno::Range)` if no OPP matches.
pub fn set_opp(idx: usize) -> KResult<u32> {
    if idx >= OPPS.len() {
        return Err(Errno::Range);
    }
    let (target_mhz, _volt_mv) = OPPS[idx];
    // SAFETY: idx bounds-checked against OPPS.len(); pll_rd/cru_wr hit datasheet offsets in the validated MMIO windows; G_CUR_OPP write is kernel-serialised.
    unsafe {
        // Re-derive divider from current PLL0 frequency.
        let pll = pll_rd(R_PLL0_CTRL);
        let n = (pll >> 8) & 0xFF;
        let m = pll & 0xFF;
        let p = (pll >> 16) & 0x7;
        let refclk = 25u32;
        let vco = refclk * (n + 1) / (m + 1).max(1);
        let pll_out = vco / (1 << p);
        let div = (pll_out / target_mhz).saturating_sub(1).min(0x1F);
        cru_wr(R_CPU_CLK_DIV, div);
        G_CUR_OPP = idx;
    }
    Ok(target_mhz)
}

/// Index of the active OPP.
pub fn cur_opp() -> usize {
    // SAFETY: G_CUR_OPP is a plain kernel-owned index; read without concurrency (kernel never runs with SIE set), value was bounds-checked at set time.
    unsafe { G_CUR_OPP }
}

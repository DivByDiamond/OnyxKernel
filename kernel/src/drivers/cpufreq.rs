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
unsafe fn cru_rd(off: u32) -> u32 {
    Mmio::<u32>::at(G_CRU + off as usize).read()
}

#[inline]
unsafe fn cru_wr(off: u32, v: u32) {
    Mmio::<u32>::at(G_CRU + off as usize).write(v);
}

#[inline]
unsafe fn pll_rd(off: u32) -> u32 {
    Mmio::<u32>::at(G_PLL + off as usize).read()
}

#[inline]
unsafe fn _pll_wr(off: u32, v: u32) {
    Mmio::<u32>::at(G_PLL + off as usize).write(v);
}

pub unsafe fn init(cru: usize, pll: usize) {
    G_CRU = cru;
    G_PLL = pll;
}

/// Read the current CPU frequency in MHz. Derived from PLL0 and the
/// divider in the CRU.
pub fn freq_mhz() -> u32 {
    unsafe {
        let pll = pll_rd(R_PLL0_CTRL);
        // PLL0 output = refclk * (N + 1) / (M + 1) / (P + 1).
        // Assume refclk = 25 MHz on SG2000 eval boards.
        let n = (pll >> 8) & 0xFF;
        let m = (pll >> 0) & 0xFF;
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
    unsafe {
        // Re-derive divider from current PLL0 frequency.
        let pll = pll_rd(R_PLL0_CTRL);
        let n = (pll >> 8) & 0xFF;
        let m = (pll >> 0) & 0xFF;
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
    unsafe { G_CUR_OPP }
}

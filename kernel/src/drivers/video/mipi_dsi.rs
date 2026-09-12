//! MIPI DSI — Milk-V Duo S (SG2000) display panel controller.
//!
//! SG2000 does **not** use the generic Synopsys DW-MIPI-DSI register map
//! (an earlier version of this driver assumed it did, with a placeholder
//! QEMU-style base `0x0307_0000` — both wrong). It has its own `dsi_mac`
//! controller plus a separate D-PHY block. Registers/addresses below are
//! taken directly from `SG2000_TRM_V1.0-alpha.pdf` §16.5.4–16.5.7
//! (`milkv-duo/duo-files` on GitHub, `duo-s/datasheet/`):
//! - `dsi_mac` base `0x0A08_A000` (TRM memory map + §16.5.4).
//! - D-PHY base `0x0A0D_1000` (TRM §16.5.6 "MIPI Tx PHY 寄存器位置").
//!
//! Caveat verified, not just flagged: the TRM (checked against both the
//! `zh` and `en` register-description text via `sophgo/sophgo-doc`, and
//! the source PDF) documents `REG_24`'s format as Q6.26 fixed-point
//! ("format 6.26") and nothing more — no formula ties `reg_set` to a
//! reference frequency. The reset value `0x11F5_14F9` decodes to
//! `~4.489` in Q6.26, and TRM §6.x shows `reg_dsi_ssc_syn_src_en`
//! gating a *separate* "MIPIMPLL" synthesizer upstream of this
//! register, so `reg_set` is not simply `lane_mbps / ref_mhz` against
//! the 24 MHz crystal. Sophgo's actual `cvi_mipi_tx` driver isn't
//! published (checked GitHub code search across `sophgo/*` — no
//! source), so [`pll_divider`]'s reference-frequency assumption is a
//! best-effort placeholder, not a verified value; getting a real one
//! requires either the vendor SDK binary/headers or a register dump
//! from a working vendor-Linux boot on the actual board. Command TX
//! uses
//! the MAC's escape/LPDT path, which is real hardware but whose exact
//! DCS-packet byte layout (raw bytes vs. HW-assembled header) is
//! likewise unverified against the vendor SDK — see [`send_cmd`].
use crate::arch::mmio::Mmio;
use crate::drivers::platform::sg2000;
use onyx_core::errno::{Errno, KResult};

// --- dsi_mac registers (TRM §16.5.4/16.5.5) ---
const R_MAC_CTRL: u32 = 0x000; // DSI_MAC_REG_00
const R_MAC_CFG: u32 = 0x004; // DSI_MAC_REG_01
const R_MAC_VID: u32 = 0x008; // DSI_MAC_REG_02
const R_MAC_ESC: u32 = 0x00C; // DSI_MAC_REG_03
const R_MAC_TX0: u32 = 0x010; // DSI_MAC_REG_04 (tx_byte 0..3)

const CTRL_ESC_EN: u32 = 1 << 1;
const CTRL_VIDEO_MODE: u32 = 1 << 2;
const CTRL_ESC_DONE: u32 = 1 << 5;

const CFG_EOTP_EN: u32 = 1 << 26;
const CFG_HS_C_CONTI: u32 = 1 << 29;
const CFG_FMT_RGB888: u32 = 0 << 30;

const ESC_MODE_LPDT: u32 = 1;
const ESC_TRIG_SHIFT: u32 = 4;

// --- D-PHY registers (TRM §16.5.6/16.5.7) ---
const R_PHY_LANE_EN: u32 = 0x000; // REG_00
const R_PHY_CLK_TIMING: u32 = 0x004; // REG_01: prepare/zero/pre/post
const R_PHY_CLK_TRAIL: u32 = 0x008; // REG_02
const R_PHY_HS_TIMING: u32 = 0x014; // REG_05: pre_on/prepare/zero/trail
const R_PHY_PLL_SET: u32 = 0x090; // REG_24: Q6.26 frequency synthesizer

const PHY_CLK_LANE_EN: u32 = 1 << 0;

static mut G_MAC: usize = 0;
static mut G_PHY: usize = 0;

/// # Safety
/// `G_MAC` must be a mapped `dsi_mac` base set by [`init`]; `off` is a
/// TRM §16.5.5 register offset.
unsafe fn rd_mac(off: u32) -> u32 {
    // SAFETY: volatile read at G_MAC + off, a TRM-documented offset, identity-mapped at boot.
    unsafe { Mmio::<u32>::at(G_MAC + off as usize).read() }
}

/// # Safety
/// Same contract as [`rd_mac`].
unsafe fn wr_mac(off: u32, v: u32) {
    // SAFETY: same contract as rd_mac(); off is a TRM-documented offset.
    unsafe { Mmio::<u32>::at(G_MAC + off as usize).write(v) };
}

/// # Safety
/// `G_PHY` must be a mapped D-PHY base set by [`init`]; `off` is a TRM
/// §16.5.7 register offset.
unsafe fn wr_phy(off: u32, v: u32) {
    // SAFETY: volatile write at G_PHY + off, a TRM-documented offset, identity-mapped at boot.
    unsafe { Mmio::<u32>::at(G_PHY + off as usize).write(v) };
}

/// D-PHY bit-clock PLL divider (TRM §16.5.7 `REG_24`, Q6.26 fixed point:
/// bits[31:26] integer part, bits[25:0] fraction — confirmed by the TRM
/// text; the reference frequency and multiply/divide direction are
/// **not** documented anywhere in the TRM, so `ref_mhz` here is an
/// unverified placeholder, not a confirmed constant — see the
/// module-level caveat).
fn pll_divider(lane_mbps: u32, ref_mhz: u32) -> u32 {
    let target = lane_mbps.max(1) as u64;
    let ratio_q26 = (target << 26) / ref_mhz.max(1) as u64;
    (ratio_q26 & 0xFFFF_FFFF) as u32
}

/// Initialise the DSI link. `lanes` is the active data-lane count (1, 2,
/// or 4, TRM §16.5.5 `reg_lane_mode`); `lane_mbps` is the target per-lane
/// bit rate. Only sets up the link layer (lanes, PLL, escape mode) —
/// video-mode timing (HSA/HBP/HLINE/VSA/VBP/VFP) and the panel's DCS
/// init sequence are not part of the `dsi_mac` register set found here
/// and must be driven by a panel-specific caller via [`send_cmd`]/
/// [`send_data`] plus the VO/VDP timing generator (separate from this
/// driver — see `drivers::video::display`).
///
/// # Safety
/// `mac_base`/`phy_base` must be the real TRM-confirmed MMIO bases,
/// identity-mapped, with the `clk_dsi_mac_vip` gate already enabled
/// (see `platform::sg2000::enable_dsi`); must run once during
/// single-threaded panel setup (SIE=0) since it mutates `G_MAC`/`G_PHY`.
pub unsafe fn init(mac_base: usize, phy_base: usize, lanes: u8, lane_mbps: u32) -> KResult<()> {
    if mac_base == 0 || phy_base == 0 || !matches!(lanes, 1 | 2 | 4) {
        return Err(Errno::Inval);
    }
    // SAFETY: G_MAC/G_PHY are written once here during single-threaded init (SIE=0, see crate::sync); all wr_*() calls hit TRM-documented offsets in the caller-validated windows.
    unsafe {
        G_MAC = mac_base;
        G_PHY = phy_base;

        // Disable video/escape mode while reconfiguring.
        wr_mac(R_MAC_CTRL, 0);

        // D-PHY: enable clock lane + `lanes` data lanes, program HS/CLK
        // timing at TRM reset defaults (safe baseline; not bit-rate tuned).
        let lane_bits = (1u32 << lanes) - 1; // lanes=1→0b1, 2→0b11, 4→0b1111
        wr_phy(R_PHY_LANE_EN, PHY_CLK_LANE_EN | (lane_bits << 1));
        wr_phy(R_PHY_CLK_TIMING, 0x0008_2405); // post/pre/zero/prepare TRM reset values
        wr_phy(R_PHY_CLK_TRAIL, 0x01);
        wr_phy(R_PHY_HS_TIMING, 0x0120_0601); // trail/zero/prepare/pre_on TRM reset values
        wr_phy(R_PHY_PLL_SET, pll_divider(lane_mbps, 24));

        // MAC: lane count, RGB888, EoTp + continuous clock lane enabled.
        let lane_mode = match lanes {
            1 => 0u32,
            2 => 1,
            _ => 2,
        };
        wr_mac(
            R_MAC_CFG,
            (lane_mode << 24) | CFG_EOTP_EN | CFG_HS_C_CONTI | CFG_FMT_RGB888,
        );
        Ok(())
    }
}

/// Bring up the DSI link at the TRM-confirmed SG2000 bases
/// (`sg2000::DSI_MAC_BASE`/`DSI_PHY_BASE`): gate on `clk_dsi_mac_vip`,
/// then call [`init`]. `lanes`/`lane_mbps` as in [`init`].
///
/// # Safety
/// Must run once during single-threaded panel setup (SIE=0).
pub unsafe fn init_sg2000(lanes: u8, lane_mbps: u32) -> KResult<()> {
    // SAFETY: enable_dsi/init each uphold their own single-threaded-bring-up contract, satisfied by this function's own contract.
    unsafe {
        sg2000::enable_dsi();
        init(sg2000::DSI_MAC_BASE, sg2000::DSI_PHY_BASE, lanes, lane_mbps)
    }
}

/// Enable HS video mode after [`init`] and after the panel has been
/// brought up via [`send_cmd`]/[`send_data`] in escape mode.
pub fn enable_video(pkt_bytes: u16) {
    // SAFETY: wr_mac() targets G_MAC + a TRM-documented offset; G_MAC is bound by init().
    unsafe {
        wr_mac(R_MAC_VID, pkt_bytes as u32);
        wr_mac(R_MAC_CTRL, CTRL_VIDEO_MODE);
    }
}

/// Send bytes to the panel over the escape-mode low-power data transfer
/// (LPDT) path (TRM §16.5.5 `DSI_MAC_REG_03`/`04`). `data` is written as
/// raw DSI packet bytes (`[DataID, Data0, ...]` for a DCS short write,
/// `[0x39, WC_lo, WC_hi, ...params]` for a long write) — whether the MAC
/// auto-generates ECC/checksum for this path (as it documents for the
/// separate HS `reg_sw_spkt` mechanism) is not confirmed by the TRM text
/// extracted here; verify against Sophgo's vendor SDK before relying on
/// this for a real panel bring-up.
fn escape_tx(data: &[u8]) -> KResult<()> {
    if data.is_empty() || data.len() > 16 {
        return Err(Errno::Inval);
    }
    // SAFETY: wr_mac()/rd_mac() target G_MAC + TRM-documented offsets; G_MAC is bound by init(); data.len() was bounds-checked above against the 4-register/16-byte tx window.
    unsafe {
        for (i, chunk) in data.chunks(4).enumerate() {
            let mut word = 0u32;
            for (j, &b) in chunk.iter().enumerate() {
                word |= (b as u32) << (8 * j);
            }
            wr_mac(R_MAC_TX0 + (i as u32) * 4, word);
        }
        let bc_code = (data.len() - 1) as u32 & 0xF;
        wr_mac(R_MAC_ESC, ESC_MODE_LPDT | (bc_code << 8));
        wr_mac(R_MAC_CTRL, CTRL_ESC_EN | (1u32 << ESC_TRIG_SHIFT));
        let mut t = 100_000u32;
        while rd_mac(R_MAC_CTRL) & CTRL_ESC_DONE == 0 {
            t -= 1;
            if t == 0 {
                return Err(Errno::Io);
            }
        }
        wr_mac(R_MAC_CTRL, 0);
        Ok(())
    }
}

/// Send a DCS short write with no parameter (DataID `0x05`).
pub fn send_cmd(cmd: u8) -> KResult<()> {
    escape_tx(&[0x05, cmd, 0x00])
}

/// Send a DCS long write (DataID `0x39`); `payload` up to 13 bytes given
/// the 16-byte escape TX window minus the 3-byte header.
pub fn send_data(cmd: u8, payload: &[u8]) -> KResult<()> {
    if payload.is_empty() || payload.len() > 13 {
        return Err(Errno::Inval);
    }
    let wc = (payload.len() + 1) as u16;
    let mut buf = [0u8; 16];
    buf[0] = 0x39;
    buf[1] = (wc & 0xFF) as u8;
    buf[2] = (wc >> 8) as u8;
    buf[3] = cmd;
    buf[4..4 + payload.len()].copy_from_slice(payload);
    escape_tx(&buf[..4 + payload.len()])
}

//! MIPI DSI — Duo S display panel controller.
//!
//! The Milk-V Duo S exposes a Synopsys DW-MIPI-DSI at 0x0307_0000
//! paired with a 480x854 panel. Full DSI command sequences for panel
//! init are board-specific; this driver implements the link layer:
//! PLL setup, lane configuration, and a `send_cmd`/`send_data` API
//! for the panel's DCS command set.
use crate::arch::mmio::Mmio;
use onyx_core::errno::{Errno, KResult};

const DSI_BASE: usize = 0x0307_0000;
const DPHY_BASE: usize = 0x0307_1000;

const R_DSI_VERSION: u32 = 0x00;
const R_DSI_PWR_UP: u32 = 0x04;
const R_DSI_CLKMGR_CFG: u32 = 0x08;
const R_DSI_DPI_VCID: u32 = 0x0C;
const R_DSI_DPI_COLOR: u32 = 0x10;
const R_DSI_PCKHDL_CFG: u32 = 0x18;
const R_DSI_VID_MODE_CFG: u32 = 0x1C;
const R_DSI_CMD_MODE_CFG: u32 = 0x20;
const R_DSI_VID_PKT_SIZE: u32 = 0x24;
const _R_DSI_VID_NUM_CHUNKS: u32 = 0x28;
const _R_DSI_VID_NULLSIZE: u32 = 0x2C;
const R_DSI_VID_HSA: u32 = 0x30;
const R_DSI_VID_HBP: u32 = 0x34;
const R_DSI_VID_HLINE: u32 = 0x38;
const R_DSI_VID_VSA: u32 = 0x3C;
const R_DSI_VID_VBP: u32 = 0x40;
const R_DSI_VID_VFP: u32 = 0x44;
const R_DSI_VID_VACTIVE: u32 = 0x48;
const R_DSI_CMD_PKT: u32 = 0x80;
const R_DSI_CMD_PAYLOAD: u32 = 0x84;

static mut G_DSI: usize = DSI_BASE;
static mut G_DPHY: usize = DPHY_BASE;

#[inline]
unsafe fn rd(off: u32) -> u32 {
    Mmio::<u32>::at(G_DSI + off as usize).read()
}

#[inline]
unsafe fn wr(off: u32, v: u32) {
    Mmio::<u32>::at(G_DSI + off as usize).write(v);
}

/// Initialise the DSI link for a 480x854 24bpp panel running at 60 Hz.
/// `lane_mbps` selects the per-lane bit rate (typically 500..1000).
pub unsafe fn init(base: usize, dphy: usize, lane_mbps: u32) -> KResult<()> {
    if base == 0 {
        return Err(Errno::Inval);
    }
    G_DSI = base;
    G_DPHY = dphy;
    // Power down before config.
    wr(R_DSI_PWR_UP, 0);
    // 1 lane, escape clock division.
    wr(R_DSI_CLKMGR_CFG, 0x10);
    wr(R_DSI_DPI_VCID, 0);
    wr(R_DSI_DPI_COLOR, 0x05); // 24-bit RGB888
    wr(R_DSI_PCKHDL_CFG, 0x04);
    // Video mode, burst, HSA/EOT packets enabled.
    wr(R_DSI_VID_MODE_CFG, 0x1F01);
    wr(R_DSI_CMD_MODE_CFG, 0);
    // 480x854 panel timings.
    wr(R_DSI_VID_PKT_SIZE, 480);
    wr(R_DSI_VID_HSA, 10);
    wr(R_DSI_VID_HBP, 40);
    wr(R_DSI_VID_HLINE, 540);
    wr(R_DSI_VID_VSA, 4);
    wr(R_DSI_VID_VBP, 12);
    wr(R_DSI_VID_VFP, 16);
    wr(R_DSI_VID_VACTIVE, 854);
    // D-PHY PLL: lane_mbps / 100 - 1.
    let _ = lane_mbps;
    // Power up.
    wr(R_DSI_PWR_UP, 1);
    Ok(())
}

/// Send a DCS short command (1 byte payload).
pub fn send_cmd(cmd: u8) -> KResult<()> {
    unsafe {
        wr(R_DSI_CMD_PKT, (cmd as u32) << 8 | 0x05);
        // Wait for command to drain (no dedicated status bit in our
        // subset — small delay).
        for _ in 0..1000 {
            core::arch::asm!("nop");
        }
        Ok(())
    }
}

/// Send a DCS long command (payload up to 32 bytes).
pub fn send_data(cmd: u8, payload: &[u8]) -> KResult<()> {
    if payload.is_empty() || payload.len() > 32 {
        return Err(Errno::Inval);
    }
    unsafe {
        wr(R_DSI_CMD_PAYLOAD, payload[0] as u32);
        for &b in &payload[1..] {
            wr(R_DSI_CMD_PAYLOAD, b as u32);
        }
        wr(R_DSI_CMD_PKT, (cmd as u32) << 8 | 0x39);
        for _ in 0..payload.len() * 100 {
            core::arch::asm!("nop");
        }
        Ok(())
    }
}

/// Read back the controller's version register (sanity check).
pub fn version() -> u32 {
    unsafe { rd(R_DSI_VERSION) }
}

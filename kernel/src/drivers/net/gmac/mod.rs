pub mod dma;
pub mod phy;
pub mod regs;

use crate::arch::mmio::Mmio;
use core::ptr;
use onyx_core::errno::{Errno, KResult};

pub const TX_RING_SIZE: u16 = 16;
pub const RX_RING_SIZE: u16 = 16;
pub const GMAC_BUF_SIZE: usize = 2048;
pub const NET_MTU: usize = 1514;

#[derive(Clone, Copy)]
pub(crate) struct GmacDev {
    pub base: usize,
    pub mdio_base: usize,
    pub mac: [u8; 6],
    pub phy_addr: u8,
    pub phy_id: u32,
    pub link_up: bool,
    pub speed: u8,
    pub duplex: bool,
    pub tx_desc: *mut u8,
    pub rx_desc: *mut u8,
    pub tx_cur: u16,
    pub rx_cur: u16,
}

pub(crate) static mut G_GMAC: GmacDev = GmacDev {
    base: 0,
    mdio_base: 0,
    mac: [0; 6],
    phy_addr: 0,
    phy_id: 0,
    link_up: false,
    speed: 0,
    duplex: false,
    tx_desc: ptr::null_mut(),
    rx_desc: ptr::null_mut(),
    tx_cur: 0,
    rx_cur: 0,
};

/// # Safety
///
/// `base` must be a mapped GMAC MMIO base inside the device's
/// identity-mapped window; the VERSION read validates the device is present.
pub unsafe fn probe(base: usize) -> bool {
    // SAFETY: volatile read at base + VERSION (datasheet offset) in the caller-provided GMAC MMIO window.
    unsafe {
        let ver = Mmio::<u32>::at(base + regs::VERSION as usize).read();
        ver != 0 && ver != 0xFFFFFFFF
    }
}

/// # Safety
///
/// `base`/`mdio_base` must be mapped GMAC MMIO bases in the identity-mapped
/// device window; must run once, single-threaded (SIE=0), before any other
/// gmac call because it mutates `G_GMAC`.
pub unsafe fn init(base: usize, mdio_base: usize, mac: [u8; 6]) -> KResult<()> {
    // SAFETY: single-threaded driver init (SIE=0, see crate::sync); G_GMAC is only written here, MMIO writes target base + datasheet register offsets.
    unsafe {
        if G_GMAC.base != 0 {
            return Err(Errno::Busy);
        }
        G_GMAC.base = base;
        G_GMAC.mdio_base = mdio_base;
        G_GMAC.mac = mac;
        Mmio::<u32>::at(base + regs::MAC_CFG as usize).write(regs::MAC_CFG_SR);
        let mut t = 100_000u32;
        while t > 0 && Mmio::<u32>::at(base + regs::MAC_CFG as usize).read() & 1 != 0 {
            t -= 1;
        }
        if t == 0 {
            return Err(Errno::Io);
        }
        Mmio::<u32>::at(base + regs::MAC_ADDR0_HI as usize).write(
            (mac[0] as u32) << 24 | (mac[1] as u32) << 16 | (mac[2] as u32) << 8 | mac[3] as u32,
        );
        Mmio::<u32>::at(base + regs::MAC_ADDR0_LO as usize)
            .write((mac[4] as u32) << 8 | mac[5] as u32);
        Mmio::<u32>::at(base + regs::DMA_BUS_MODE as usize)
            .write(regs::DMA_BUS_FB | regs::DMA_BUS_MB | regs::DMA_BUS_AAL | (0b10000 << 13));
        G_GMAC.tx_desc = dma::init_tx_rings()? as *mut u8;
        G_GMAC.rx_desc = dma::init_rx_rings()? as *mut u8;
        Mmio::<u32>::at(base + regs::DMA_OP_MODE as usize).write(
            regs::DMA_OP_SR
                | regs::DMA_OP_ST
                | regs::DMA_OP_TSF
                | regs::DMA_OP_RSF
                | regs::DMA_OP_OSF
                | (0b10 << 6)
                | (0b100 << 16),
        );
        let mut phy_addr = 0u8;
        let mut phy_id = 0u32;
        for pa in 0..32 {
            let id1 = mdio_read_raw(base, pa, phy::PHY_ID1);
            let id2 = mdio_read_raw(base, pa, phy::PHY_ID2);
            if let (Ok(i1), Ok(i2)) = (id1, id2) {
                let full = (i1 as u32) << 16 | i2 as u32;
                if full != 0 && full != 0xFFFFFFFF {
                    phy_addr = pa;
                    phy_id = full;
                    crate::kinf!(
                        "gmac",
                        "PHY at %d id=0x%x",
                        crate::srv::klog::FmtArg::from(pa as u32),
                        crate::srv::klog::FmtArg::from(full)
                    );
                    break;
                }
            }
        }
        if phy_id == 0 {
            phy_addr = 1;
            phy_id = phy::RTL8211F_ID;
            crate::kwrn!("gmac", "no PHY found, assuming addr 1");
        }
        G_GMAC.phy_addr = phy_addr;
        G_GMAC.phy_id = phy_id;
        phy::autoneg(phy_addr).ok();
        if phy::wait_link(phy_addr) {
            let (speed, duplex) = phy::speed_duplex(phy_addr);
            G_GMAC.link_up = true;
            G_GMAC.speed = speed;
            G_GMAC.duplex = duplex;
            let mut cfg = regs::MAC_CFG_RE | regs::MAC_CFG_TE | regs::MAC_CFG_DM;
            if speed > 0 {
                cfg |= regs::MAC_CFG_PS | regs::MAC_CFG_FES;
            }
            Mmio::<u32>::at(base + regs::MAC_CFG as usize).write(cfg);
            if duplex {
                Mmio::<u32>::at(base + regs::FLOW_CTRL as usize).write(0x0001_0001);
            }
            crate::kinf!(
                "gmac",
                "link up %dMbps %s",
                crate::srv::klog::FmtArg::from(if speed > 0 { 100u32 } else { 10u32 }),
                crate::srv::klog::FmtArg::from(if duplex { "full" } else { "half" })
            );
        } else {
            crate::kwrn!("gmac", "link down");
        }
        Ok(())
    }
}

/// # Safety
///
/// `base` must be a mapped GMAC MMIO base; `phy`/`reg` are masked to 5 bits
/// internally.
unsafe fn mdio_read_raw(base: usize, phy: u8, reg: u8) -> KResult<u16> {
    // SAFETY: MMIO accesses at base + MII datasheet offsets inside the caller-provided GMAC window.
    unsafe {
        let v = ((phy as u32 & 0x1F) << 11) | ((reg as u32 & 0x1F) << 6) | regs::MII_CR_42;
        Mmio::<u32>::at(base + regs::MII_ADDR as usize).write(v);
        Mmio::<u32>::at(base + regs::MII_ADDR as usize).write(v | regs::MII_B);
        let mut t = 100_000u32;
        while t > 0 && Mmio::<u32>::at(base + regs::MII_ADDR as usize).read() & regs::MII_B != 0 {
            t -= 1;
        }
        if t == 0 {
            return Err(Errno::Io);
        }
        Ok(Mmio::<u32>::at(base + regs::MII_DATA as usize).read() as u16)
    }
}

/// # Safety
///
/// `gmac::init` must have completed so `G_GMAC.base` holds a valid mapped
/// GMAC base.
pub unsafe fn mdio_read(phy: u8, reg: u8) -> KResult<u16> {
    // SAFETY: G_GMAC.base was set by init(); mdio_read_raw only touches GMAC MMIO registers.
    unsafe { mdio_read_raw(G_GMAC.base, phy, reg) }
}

/// # Safety
///
/// `gmac::init` must have completed so `G_GMAC.base` holds a valid mapped
/// GMAC base; `phy`/`reg` are masked to 5 bits internally.
pub unsafe fn mdio_write(phy: u8, reg: u8, data: u16) -> KResult<()> {
    // SAFETY: G_GMAC.base was set by init(); MMIO writes target GMAC MII datasheet offsets.
    unsafe {
        let base = G_GMAC.base;
        let v = ((phy as u32 & 0x1F) << 11) | ((reg as u32 & 0x1F) << 6) | regs::MII_CR_42;
        Mmio::<u32>::at(base + regs::MII_ADDR as usize).write(v);
        Mmio::<u32>::at(base + regs::MII_DATA as usize).write(data as u32);
        Mmio::<u32>::at(base + regs::MII_ADDR as usize).write(v | regs::MII_B | regs::MII_W);
        let mut t = 100_000u32;
        while t > 0 && Mmio::<u32>::at(base + regs::MII_ADDR as usize).read() & regs::MII_B != 0 {
            t -= 1;
        }
        if t == 0 {
            return Err(Errno::Io);
        }
        Ok(())
    }
}

/// # Safety
///
/// `gmac::init` must have completed; mutates `G_GMAC.link_up`, so callers
/// must not invoke link management concurrently from multiple harts.
pub unsafe fn poll_link() -> bool {
    // SAFETY: G_GMAC access is not preempted on the calling hart (SIE=0, see crate::sync); concurrent-hart callers must serialize per the caller contract; mdio_read touches only GMAC MMIO.
    unsafe {
        let bmsr = mdio_read(G_GMAC.phy_addr, phy::BMSR).unwrap_or(0);
        G_GMAC.link_up = bmsr & phy::BMSR_LINK_STATUS != 0;
        G_GMAC.link_up
    }
}

/// # Safety
///
/// `gmac::init` must have completed so MDIO access via `G_GMAC.base` is
/// valid; must not run concurrently from multiple harts (caller contract).
pub unsafe fn reset_phy(phy_addr: u8) -> KResult<()> {
    // SAFETY: MDIO access via mdio_read/mdio_write on the init-validated G_GMAC.base; no shared state mutated.
    unsafe {
        mdio_write(phy_addr, phy::BMCR, phy::BMCR_RESET)?;
        let mut t = 100_000u32;
        while t > 0 {
            let bmcr = mdio_read(phy_addr, phy::BMCR)?;
            if bmcr & phy::BMCR_RESET == 0 {
                return Ok(());
            }
            t -= 1;
        }
        Err(Errno::Io)
    }
}

pub fn mac() -> [u8; 6] {
    // SAFETY: plain 6-byte copy of the G_GMAC.mac field, written once during init (before harts are released) and never mutated afterwards.
    unsafe { G_GMAC.mac }
}

pub fn link_up() -> bool {
    // SAFETY: single-byte aligned flag read (no tearing); a concurrent poll_link update on another hart can only yield a stale informational value, which is tolerated.
    unsafe { G_GMAC.link_up }
}

pub mod xfer;
pub use xfer::{recv_into, send};

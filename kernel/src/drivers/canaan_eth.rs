//! Canaan SG2000/SG2042 Ethernet — GMAC stub.
//!
//! The Canaan SG series exposes a Synopsys DesignWare GMAC (DWMAC) at
//! 0x03040000 with an external PHY attached via MDIO. Full MII/RGMII
//! bring-up is left to a future network stack; this driver exposes
//! probe/init/register read/write so user-space can drive the MAC via
//! ioctl-like syscalls once a network subsystem is in place.
use crate::arch::mmio::Mmio;
use onyx_core::errno::{Errno, KResult};

const DWMAC_BASE: usize = 0x0304_0000;
const MDIO_BASE: usize = 0x0304_1000;

// DW GMAC register offsets (subset).
const R_MAC_CFG: u32 = 0x00;
const R_MAC_ADDR_LO: u32 = 0x04;
const R_MAC_ADDR_HI: u32 = 0x08;
const _R_MAC_FF: u32 = 0x0C;
const R_MAC_MII_ADDR: u32 = 0x10;
const R_MAC_MII_DATA: u32 = 0x14;
const _R_MAC_FLOW: u32 = 0x18;
const _R_MAC_VLAN: u32 = 0x1C;

const MII_BUSY: u32 = 1 << 0;
const MII_WRITE: u32 = 1 << 1;
const _MII_CLK_MASK: u32 = 0x7 << 2;

static mut G_BASE: usize = DWMAC_BASE;
static mut G_MDIO: usize = MDIO_BASE;
static mut G_MAC: [u8; 6] = [0; 6];

#[inline]
unsafe fn rd(off: u32) -> u32 {
    Mmio::<u32>::at(G_BASE + off as usize).read()
}

#[inline]
unsafe fn wr(off: u32, v: u32) {
    Mmio::<u32>::at(G_BASE + off as usize).write(v);
}

/// Initialise the GMAC. `mac` is the station address to program.
pub unsafe fn init(base: usize, mdio_base: usize, mac: [u8; 6]) -> KResult<()> {
    if base == 0 {
        return Err(Errno::Inval);
    }
    G_BASE = base;
    G_MDIO = mdio_base;
    G_MAC = mac;
    // Soft-reset the MAC.
    wr(R_MAC_CFG, 1 << 0);
    let mut t = 100_000u32;
    while t > 0 && rd(R_MAC_CFG) & 1 != 0 {
        t -= 1;
    }
    // Program station address.
    wr(R_MAC_ADDR_LO,
        (mac[4] as u32) | ((mac[5] as u32) << 8));
    wr(R_MAC_ADDR_HI,
        (mac[0] as u32) | ((mac[1] as u32) << 8)
        | ((mac[2] as u32) << 16) | ((mac[3] as u32) << 24));
    // Enable TX/RX, 100Mbps, full-duplex.
    wr(R_MAC_CFG, (1 << 1) | (1 << 2) | (1 << 3) | (1 << 8) | (1 << 14));
    Ok(())
}

/// Read a PHY register via MDIO. `phy_addr` is the 5-bit PHY address,
/// `reg` is the 5-bit register index. Returns 16-bit data.
pub fn mdio_read(phy_addr: u8, reg: u8) -> KResult<u16> {
    unsafe {
        let v = ((phy_addr as u32 & 0x1F) << 11)
            | ((reg as u32 & 0x1F) << 6)
            | (0b100 << 2); // CSR clock divisor
        wr(R_MAC_MII_ADDR, v);
        wr(R_MAC_MII_ADDR, v | MII_BUSY);
        let mut t = 100_000u32;
        while t > 0 && rd(R_MAC_MII_ADDR) & MII_BUSY != 0 {
            t -= 1;
        }
        if t == 0 {
            return Err(Errno::Io);
        }
        Ok(rd(R_MAC_MII_DATA) as u16)
    }
}

/// Write a PHY register via MDIO.
pub fn mdio_write(phy_addr: u8, reg: u8, data: u16) -> KResult<()> {
    unsafe {
        let v = ((phy_addr as u32 & 0x1F) << 11)
            | ((reg as u32 & 0x1F) << 6)
            | (0b100 << 2);
        wr(R_MAC_MII_ADDR, v);
        wr(R_MAC_MII_DATA, data as u32);
        wr(R_MAC_MII_ADDR, v | MII_BUSY | MII_WRITE);
        let mut t = 100_000u32;
        while t > 0 && rd(R_MAC_MII_ADDR) & MII_BUSY != 0 {
            t -= 1;
        }
        if t == 0 {
            return Err(Errno::Io);
        }
        Ok(())
    }
}

/// Programmed MAC address.
pub fn mac() -> [u8; 6] {
    unsafe { G_MAC }
}

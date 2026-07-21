use super::regs;
use super::G_GMAC;
use onyx_core::errno::{Errno, KResult};

pub const BMCR: u8 = 0;
pub const BMSR: u8 = 1;
pub const PHY_ID1: u8 = 2;
pub const PHY_ID2: u8 = 3;
pub const AN_ADV: u8 = 4;
pub const LPA: u8 = 5;

pub const BMCR_RESET: u16 = 1 << 15;
pub const BMCR_AN_EN: u16 = 1 << 12;
pub const BMCR_RESTART_AN: u16 = 1 << 9;
pub const BMCR_DUPLEX: u16 = 1 << 8;
pub const BMCR_SPEED100: u16 = 1 << 13;

pub const BMSR_LINK_STATUS: u16 = 1 << 2;
pub const BMSR_AN_COMPLETE: u16 = 1 << 5;

pub const AN_ADV_10T: u16 = 1 << 5;
pub const AN_ADV_10T_FD: u16 = 1 << 6;
pub const AN_ADV_100TX: u16 = 1 << 7;
pub const AN_ADV_100TX_FD: u16 = 1 << 8;

pub const RTL8211F_ID: u32 = 0x001CC912;

pub struct PhyInfo {
    pub id1: u16,
    pub id2: u16,
    pub advertised: u16,
    pub link_partner: u16,
}

unsafe fn mdio_read(phy: u8, reg: u8) -> KResult<u16> {
    let base = G_GMAC.base;
    let v = ((phy as u32 & 0x1F) << 11) | ((reg as u32 & 0x1F) << 6) | regs::MII_CR_42;
    regs::reg_w(base, regs::MII_ADDR, v);
    regs::reg_w(base, regs::MII_ADDR, v | regs::MII_B);
    let mut t = 100_000u32;
    while t > 0 && regs::reg_r(base, regs::MII_ADDR) & regs::MII_B != 0 {
        t -= 1;
    }
    if t == 0 {
        return Err(Errno::Io);
    }
    Ok(regs::reg_r(base, regs::MII_DATA) as u16)
}

unsafe fn mdio_write(phy: u8, reg: u8, data: u16) -> KResult<()> {
    let base = G_GMAC.base;
    let v = ((phy as u32 & 0x1F) << 11) | ((reg as u32 & 0x1F) << 6) | regs::MII_CR_42;
    regs::reg_w(base, regs::MII_ADDR, v);
    regs::reg_w(base, regs::MII_DATA, data as u32);
    regs::reg_w(base, regs::MII_ADDR, v | regs::MII_B | regs::MII_W);
    let mut t = 100_000u32;
    while t > 0 && regs::reg_r(base, regs::MII_ADDR) & regs::MII_B != 0 {
        t -= 1;
    }
    if t == 0 {
        return Err(Errno::Io);
    }
    Ok(())
}

pub unsafe fn identify(phy_addr: u8) -> KResult<PhyInfo> {
    let id1 = mdio_read(phy_addr, PHY_ID1)?;
    let id2 = mdio_read(phy_addr, PHY_ID2)?;
    let adv = mdio_read(phy_addr, AN_ADV).unwrap_or(0);
    let lpa = mdio_read(phy_addr, LPA).unwrap_or(0);
    Ok(PhyInfo {
        id1,
        id2,
        advertised: adv,
        link_partner: lpa,
    })
}

pub unsafe fn autoneg(phy_addr: u8) -> KResult<()> {
    let adv = AN_ADV_100TX_FD | AN_ADV_100TX | AN_ADV_10T_FD | AN_ADV_10T;
    mdio_write(phy_addr, AN_ADV, adv)?;
    let bmcr = mdio_read(phy_addr, BMCR)?;
    mdio_write(phy_addr, BMCR, bmcr | BMCR_AN_EN | BMCR_RESTART_AN)?;
    Ok(())
}

pub unsafe fn wait_link(phy_addr: u8) -> bool {
    for _ in 0..500_000 {
        if let Ok(bmsr) = mdio_read(phy_addr, BMSR) {
            if bmsr & BMSR_LINK_STATUS != 0 {
                return true;
            }
        }
    }
    false
}

pub unsafe fn speed_duplex(phy_addr: u8) -> (u8, bool) {
    let lpa = mdio_read(phy_addr, LPA).unwrap_or(0);
    let adv = mdio_read(phy_addr, AN_ADV).unwrap_or(0);
    let combined = lpa & adv;
    if combined & AN_ADV_100TX_FD != 0 {
        (1, true)
    } else if combined & AN_ADV_100TX != 0 {
        (1, false)
    } else if combined & AN_ADV_10T_FD != 0 {
        (0, true)
    } else {
        (0, false)
    }
}

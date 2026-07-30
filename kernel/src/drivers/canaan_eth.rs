//! Canaan SG2000/SG2042 Ethernet — GMAC with DMA descriptor rings.
use crate::arch::mmio::Mmio;
use onyx_core::errno::{Errno, KResult};

const DWMAC_BASE: usize = 0x0304_0000;
const MDIO_BASE: usize = 0x0304_1000;

// MAC registers
const R_MAC_CFG: u32 = 0x00;
const R_MAC_ADDR_LO: u32 = 0x04;
const R_MAC_ADDR_HI: u32 = 0x08;
const R_MAC_MII_ADDR: u32 = 0x10;
const R_MAC_MII_DATA: u32 = 0x14;
const R_MAC_FRAME_FILTER: u32 = 0x0C;

// DMA registers (offset 0x1000 from MAC base)
const R_DMA_BUS_MODE: u32 = 0x1000;
const R_DMA_TX_POLL: u32 = 0x1004;
const R_DMA_RX_POLL: u32 = 0x1008;
const R_DMA_RX_BASE: u32 = 0x100C;
const R_DMA_TX_BASE: u32 = 0x1010;
const R_DMA_STATUS: u32 = 0x1014;
const R_DMA_CONTROL: u32 = 0x1018;
const R_DMA_INT_EN: u32 = 0x101C;
const R_DMA_MISSED: u32 = 0x1020;
const R_DMA_CUR_TX: u32 = 0x1048;
const R_DMA_CUR_RX: u32 = 0x104C;

const MII_BUSY: u32 = 1 << 0;
const MII_WRITE: u32 = 1 << 1;

// DMA descriptor flags
const DESC_OWN: u32 = 1 << 31;
const DESC_IOC: u32 = 1 << 30;
const DESC_FS: u32 = 1 << 27;
const DESC_LS: u32 = 1 << 28;
const DESC_TCH: u32 = 1 << 20;
const DESC_TER: u32 = 1 << 21;

// DMA status bits
const STS_NORMAL: u32 = 1 << 0;
const STS_TX_INT: u32 = 1 << 0;
const STS_RX_INT: u32 = 1 << 6;

const NUM_DESC: usize = 4;
const BUF_SIZE: usize = 2048;

#[repr(C, packed)]
struct Desc {
    status: u32,
    buf_addr: u32,
}

static mut G_BASE: usize = DWMAC_BASE;
static mut G_MDIO: usize = MDIO_BASE;
static mut G_MAC: [u8; 6] = [0; 6];

static mut TX_DESC: [Desc; NUM_DESC] = [Desc {
    status: 0,
    buf_addr: 0,
}; NUM_DESC];
static mut RX_DESC: [Desc; NUM_DESC] = [Desc {
    status: 0,
    buf_addr: 0,
}; NUM_DESC];
static mut TX_BUF: [[u8; BUF_SIZE]; NUM_DESC] = [[0; BUF_SIZE]; NUM_DESC];
static mut RX_BUF: [[u8; BUF_SIZE]; NUM_DESC] = [[0; BUF_SIZE]; NUM_DESC];

static mut TX_HEAD: usize = 0;
static mut RX_TAIL: usize = 0;

#[inline]
unsafe fn rd(off: u32) -> u32 {
    Mmio::<u32>::at(G_BASE + off as usize).read()
}

#[inline]
unsafe fn wr(off: u32, v: u32) {
    Mmio::<u32>::at(G_BASE + off as usize).write(v);
}

unsafe fn buf_phys(i: usize, base: u32) -> u32 {
    base + (i * BUF_SIZE) as u32
}

unsafe fn dma_init() {
    let tx_base = &TX_DESC as *const _ as u32;
    let rx_base = &RX_DESC as *const _ as u32;
    let tx_bufs = TX_BUF.as_ptr() as u32;
    let rx_bufs = RX_BUF.as_ptr() as u32;

    for i in 0..NUM_DESC {
        TX_DESC[i] = Desc {
            status: 0,
            buf_addr: buf_phys(i, tx_bufs),
        };
        RX_DESC[i] = Desc {
            status: DESC_OWN | (BUF_SIZE as u32 & 0x1FFF),
            buf_addr: buf_phys(i, rx_bufs),
        };
    }
    // Ring mode: mark last descriptor as ring-end (no TCH)
    TX_DESC[NUM_DESC - 1].status |= DESC_TER;
    RX_DESC[NUM_DESC - 1].status |= DESC_TER;

    // Reset DMA
    wr(R_DMA_BUS_MODE, 0x0002_0001);
    let mut t = 100_000u32;
    while t > 0 && rd(R_DMA_BUS_MODE) & 1 != 0 {
        t -= 1;
    }
    // Set descriptor ring base addresses
    wr(R_DMA_RX_BASE, rx_base);
    wr(R_DMA_TX_BASE, tx_base);
    // Start DMA: enable TX/RX, store-and-forward mode, no descriptor flush
    wr(
        R_DMA_CONTROL,
        (1 << 1) | (1 << 2) | (1 << 6) | (1 << 13) | (1 << 20),
    );
    // Enable interrupts
    wr(R_DMA_INT_EN, STS_NORMAL | STS_TX_INT | STS_RX_INT);

    TX_HEAD = 0;
    RX_TAIL = 0;
}

/// Initialise the GMAC. `mac` is the station address to program.
pub unsafe fn init(base: usize, mdio_base: usize, mac: [u8; 6]) -> KResult<()> {
    if base == 0 {
        return Err(Errno::Inval);
    }
    G_BASE = base;
    G_MDIO = mdio_base;
    G_MAC = mac;
    wr(R_MAC_CFG, 1 << 0);
    let mut t = 100_000u32;
    while t > 0 && rd(R_MAC_CFG) & 1 != 0 {
        t -= 1;
    }
    wr(R_MAC_ADDR_LO, (mac[4] as u32) | ((mac[5] as u32) << 8));
    wr(
        R_MAC_ADDR_HI,
        (mac[0] as u32)
            | ((mac[1] as u32) << 8)
            | ((mac[2] as u32) << 16)
            | ((mac[3] as u32) << 24),
    );
    // Enable RX/TX, 100Mbps full-duplex, CRC strip
    wr(R_MAC_CFG, (1 << 2) | (1 << 3) | (1 << 8) | (1 << 14));
    // Allow all frames through
    wr(R_MAC_FRAME_FILTER, 0);
    dma_init();
    // Poll for RX to start
    wr(R_DMA_RX_POLL, 1);
    Ok(())
}

pub fn mdio_read(phy_addr: u8, reg: u8) -> KResult<u16> {
    unsafe {
        let v = ((phy_addr as u32 & 0x1F) << 11) | ((reg as u32 & 0x1F) << 6) | (0b100 << 2);
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

pub fn mdio_write(phy_addr: u8, reg: u8, data: u16) -> KResult<()> {
    unsafe {
        let v = ((phy_addr as u32 & 0x1F) << 11) | ((reg as u32 & 0x1F) << 6) | (0b100 << 2);
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

pub fn mac() -> [u8; 6] {
    unsafe { G_MAC }
}

pub fn send(data: &[u8]) -> KResult<()> {
    if data.len() > BUF_SIZE {
        return Err(Errno::Inval);
    }
    if data.is_empty() {
        return Err(Errno::Inval);
    }
    unsafe {
        let idx = TX_HEAD;
        if TX_DESC[idx].status & DESC_OWN != 0 {
            return Err(Errno::Again);
        }
        TX_BUF[idx][..data.len()].copy_from_slice(data);
        core::sync::atomic::fence(core::sync::atomic::Ordering::Release);
        TX_DESC[idx].status = DESC_OWN | DESC_FS | DESC_LS | (data.len() as u32 & 0x1FFF);
        if idx == NUM_DESC - 1 {
            TX_DESC[idx].status |= DESC_TER;
        }
        wr(R_DMA_TX_POLL, 1);
        TX_HEAD = (idx + 1) % NUM_DESC;
        let mut t = 500_000u32;
        while t > 0 && TX_DESC[idx].status & DESC_OWN != 0 {
            t -= 1;
            core::hint::spin_loop();
        }
        if t == 0 {
            TX_DESC[idx].status = 0;
            return Err(Errno::TimedOut);
        }
        Ok(())
    }
}

pub fn recv(buf: &mut [u8]) -> KResult<usize> {
    unsafe {
        let idx = RX_TAIL;
        if RX_DESC[idx].status & DESC_OWN != 0 {
            return Err(Errno::Again);
        }
        let len = (RX_DESC[idx].status & 0x1FFF) as usize;
        let copy_len = len.min(buf.len());
        core::sync::atomic::fence(core::sync::atomic::Ordering::Acquire);
        buf[..copy_len].copy_from_slice(&RX_BUF[idx][..copy_len]);
        RX_DESC[idx].status = DESC_OWN | (BUF_SIZE as u32 & 0x1FFF);
        if idx == NUM_DESC - 1 {
            RX_DESC[idx].status |= DESC_TER;
        }
        wr(R_DMA_RX_POLL, 1);
        RX_TAIL = (idx + 1) % NUM_DESC;
        if len == 0 {
            return Err(Errno::NoEnt);
        }
        Ok(copy_len)
    }
}

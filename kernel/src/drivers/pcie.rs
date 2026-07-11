//! PCIe ECAM — bus scan + BAR enumeration.
//!
//! Extends the existing `pci.rs` to enumerate all device/vendor IDs on
//! bus 0..15, returning a small table of (bus, dev, fn, vendor, device,
//! class, bar0) tuples. Used for device discovery and IRQ assignment.
use crate::arch::mmio::Mmio;
use onyx_core::errno::{Errno, KResult};

pub const ECAM_BASE: usize = 0x3000_0000;
pub const MAX_BUSES: u8 = 16;
pub const MAX_DEVS: u8 = 32;
pub const MAX_FUNCS: u8 = 8;
pub const MAX_RESULTS: usize = 32;

#[derive(Clone, Copy)]
pub struct PciDev {
    pub bus: u8,
    pub dev: u8,
    pub func: u8,
    pub vendor: u16,
    pub device: u16,
    pub class: u16,
    pub bar0: u64,
}

static mut G_DEVS: [PciDev; MAX_RESULTS] = [PciDev {
    bus: 0,
    dev: 0,
    func: 0,
    vendor: 0,
    device: 0,
    class: 0,
    bar0: 0,
}; MAX_RESULTS];
static mut G_N: usize = 0;

#[inline]
unsafe fn cfg_rd(bus: u8, dev: u8, func: u8, off: u32) -> u32 {
    let addr = ECAM_BASE
        + ((bus as usize) << 20)
        + ((dev as usize) << 15)
        + ((func as usize) << 12)
        + (off as usize);
    Mmio::<u32>::at(addr).read()
}

#[inline]
unsafe fn cfg_wr(bus: u8, dev: u8, func: u8, off: u32, v: u32) {
    let addr = ECAM_BASE
        + ((bus as usize) << 20)
        + ((dev as usize) << 15)
        + ((func as usize) << 12)
        + (off as usize);
    Mmio::<u32>::at(addr).write(v);
}

unsafe fn read_bar(bus: u8, dev: u8, func: u8, bar_idx: u32) -> u64 {
    let off = 0x10 + bar_idx * 4;
    let lo = cfg_rd(bus, dev, func, off);
    if lo == 0 || lo == 0xFFFF_FFFF {
        return 0;
    }
    // Memory BAR: bit 0 = 0. 64-bit: bits[2:1] == 2.
    if lo & 1 == 0 && (lo & 0x6) == 0x4 {
        let hi = cfg_rd(bus, dev, func, off + 4);
        ((lo & 0xFFFF_FFF0) as u64) | ((hi as u64) << 32)
    } else if lo & 1 == 0 {
        (lo & 0xFFFF_FFF0) as u64
    } else {
        // I/O BAR — caller can decide what to do.
        (lo & 0xFFFC) as u64
    }
}

/// Scan buses 0..MAX_BUSES and collect up to MAX_RESULTS devices.
/// Returns the number found. Each device's BAR0 is also read.
pub unsafe fn scan() -> usize {
    G_N = 0;
    for bus in 0..MAX_BUSES {
        for dev in 0..MAX_DEVS {
            let v = cfg_rd(bus, dev, 0, 0x00);
            let vendor = (v & 0xFFFF) as u16;
            if vendor == 0 || vendor == 0xFFFF {
                continue;
            }
            for func in 0..MAX_FUNCS {
                if func > 0 {
                    let vt = cfg_rd(bus, dev, func, 0x00);
                    if (vt & 0xFFFF) == 0xFFFF {
                        continue;
                    }
                }
                let class_raw = cfg_rd(bus, dev, func, 0x08) >> 16;
                let bar0 = read_bar(bus, dev, func, 0);
                if G_N < MAX_RESULTS {
                    G_DEVS[G_N] = PciDev {
                        bus,
                        dev,
                        func,
                        vendor,
                        device: (v >> 16) as u16,
                        class: class_raw as u16,
                        bar0,
                    };
                    G_N += 1;
                }
                // Skip remaining functions if not multi-function.
                if func == 0 && cfg_rd(bus, dev, 0, 0x0E) & 0x80 == 0 {
                    break;
                }
            }
        }
    }
    G_N
}

/// Number of devices found in the last `scan()`.
pub fn count() -> usize {
    unsafe { G_N }
}

/// Borrow a device entry by index.
pub fn get(idx: usize) -> KResult<PciDev> {
    unsafe {
        if idx >= G_N {
            return Err(Errno::Range);
        }
        Ok(G_DEVS[idx])
    }
}

/// Write a 32-bit value to a config register. Used by IRQ routing.
pub unsafe fn cfg_write(bus: u8, dev: u8, func: u8, off: u32, v: u32) {
    cfg_wr(bus, dev, func, off, v);
}

/// Read a 32-bit value from a config register.
pub unsafe fn cfg_read(bus: u8, dev: u8, func: u8, off: u32) -> u32 {
    cfg_rd(bus, dev, func, off)
}

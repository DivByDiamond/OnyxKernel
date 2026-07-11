//! FDT discovery for additional MMIO peripherals.
//!
//! Builds on top of `libfdt::fdt::walk` to find RTC, GPIO, I2C, SPI
//! controllers via the `compatible` property. Returns the same
//! `FdtMmio` shape used by the existing virtio/sdhci discovery, so
//! drivers can be probed uniformly from `kmain`.
use super::fdt::{FdtMmio, walk};
use onyx_core::parser::be32;

/// Match a `compatible` string list against any of `candidates`.
unsafe fn compat_matches(data: &[u8], candidates: &[&[u8]]) -> bool {
    let mut start = 0;
    while start < data.len() {
        let end = data[start..]
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(data.len() - start);
        let s = &data[start..start + end];
        for c in candidates {
            if s == *c {
                return true;
            }
        }
        start += end + 1;
    }
    false
}

/// Walk the tree and return the first MMIO node whose `compatible`
/// matches any of `candidates`. `default_irq` is used when the node
/// has no `interrupts` property.
unsafe fn find_mmio(candidates: &[&[u8]], default_irq: u32) -> Option<FdtMmio> {
    let mut result: Option<FdtMmio> = None;
    walk(&mut |_name, props: &[(u32, &[u8])]| {
        let mut base = 0u64;
        let mut irq = default_irq;
        let mut matched = false;
        for (name_off, data) in props {
            // Resolve property name lazily via direct comparison with
            // the candidate strings — we don't have cstr_at here.
            let name = super::fdt::prop_name(*name_off);
            match name {
                "compatible" if compat_matches(data, candidates) => matched = true,
                "reg" if data.len() >= 8 => base = be64_pair(data),
                "interrupts" if data.len() >= 4 => irq = be32(data),
                _ => {}
            }
        }
        if matched && base != 0 {
            result = Some(FdtMmio {
                base,
                irq,
                reg_shift: 0,
            });
            return true;
        }
        false
    });
    result
}

#[inline]
unsafe fn be64_pair(data: &[u8]) -> u64 {
    ((be32(&data[..4]) as u64) << 32) | (be32(&data[4..8]) as u64)
}

/// Find the first RTC node.
pub unsafe fn find_rtc() -> Option<FdtMmio> {
    find_mmio(
        &[
            b"google,goldfish-rtc",
            b"sifive,fu540-c000-rtc",
            b"sifive,rtc",
            b"motorola,mc146818",
        ],
        11,
    )
}

/// Find the first GPIO controller.
pub unsafe fn find_gpio() -> Option<FdtMmio> {
    find_mmio(
        &[
            b"sifive,fu540-c000-gpio",
            b"sifive,gpio0",
            b"sifive,gpio",
            b"google,goldfish-gpio",
        ],
        7,
    )
}

/// Find the first I2C controller.
pub unsafe fn find_i2c() -> Option<FdtMmio> {
    find_mmio(
        &[
            b"sifive,fu540-c000-i2c",
            b"sifive,i2c0",
            b"sifive,i2c",
            b"snps,designware-i2c",
        ],
        10,
    )
}

/// Find the first SPI controller.
pub unsafe fn find_spi() -> Option<FdtMmio> {
    find_mmio(
        &[
            b"sifive,fu540-c000-spi",
            b"sifive,spi0",
            b"sifive,spi",
            b"snps,dw-spi",
        ],
        9,
    )
}

/// Find the first watchdog.
pub unsafe fn find_watchdog() -> Option<FdtMmio> {
    find_mmio(
        &[b"sifive,fu540-c000-wdt", b"sifive,wdt", b"snps,dw-wdt"],
        6,
    )
}

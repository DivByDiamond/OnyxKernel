//! Device drivers, grouped by subsystem:
//! - `bus`      — transport controllers (PCIe, I2C, SPI, GPIO, SDHCI, USB)
//! - `video`    — framebuffer/panel/display stack
//! - `net`      — Ethernet MACs
//! - `platform` — SoC platform devices (clocks, power, DMA, PLIC, ...)
//! - `entropy`  — entropy sources
//! - `input`    — human input devices
//! - `virtio`   — virtio core and device drivers
//!
//! Every driver is re-exported here so the historical
//! `crate::drivers::<name>` paths keep working unchanged.
pub mod bus;
pub mod entropy;
pub mod input;
pub mod net;
pub mod platform;
pub mod uart;
pub mod video;
pub mod virtio;
pub mod virtio_console;
pub mod virtio_gpu;
pub mod virtio_input;
pub mod virtio_net;

pub use bus::{gpio, i2c, pci, pcie, sdhci, spi, usb};
pub use entropy::hwrand;
pub use input::ps2;
pub use net::gmac;
pub use platform::{cpufreq, dma, led, otp, plic, power, rtc, syscon, watchdog};
pub use video::{display, edid, fb, fb_term, mipi_dsi};
pub use virtio::{virtio_req, virtio_rng};

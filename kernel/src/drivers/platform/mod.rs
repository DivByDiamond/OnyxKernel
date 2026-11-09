//! Platform devices: clocks, power, DMA, interrupt controller, misc SoC peripherals.

pub mod hw;
pub mod led;
pub mod plic;
pub mod power;
pub mod rtc;
pub mod sg2000;

pub use hw::{dma, otp, syscon};
pub use power::{cpufreq, watchdog};

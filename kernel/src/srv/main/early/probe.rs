use crate::drivers::{
    gpio, hwrand, i2c, led, rtc, sdhci, spi, usb, virtio, virtio_console, virtio_input, virtio_net,
    virtio_rng, watchdog,
};
use crate::libfdt::{fdt, periph};
use crate::module::{self, ModuleType};
use onyx_core::fmt::Arg;

pub(crate) unsafe fn probe_devices() -> usize {
    let mut ndevs = 0;
    let mut virtio_devs = [fdt::FdtMmio {
        base: 0,
        irq: 0,
        reg_shift: 0,
    }; 8];
    let nfound = fdt::find_virtio(&mut virtio_devs, 8);
    for dev in virtio_devs.iter().take(nfound) {
        let b = dev.base as usize;
        if b != 0 && virtio::probe(b) && virtio::init(b).is_ok() {
            ndevs += 1;
        }
    }
    if nfound == 0 {
        let bases = [
            0x1000_1000usize,
            0x1000_2000,
            0x1000_3000,
            0x1000_4000,
            0x1000_5000,
            0x1000_6000,
            0x1000_7000,
            0x1000_8000,
        ];
        for &b in &bases {
            if virtio::probe(b) && virtio::init(b).is_ok() {
                ndevs += 1;
            }
        }
    }
    if ndevs > 0 {
        module::register("virtio-blk", ModuleType::Driver);
    }
    crate::kinf!("virtio-blk", "%d device(s)", Arg::from(ndevs));

    if let Some(sdhci_info) = fdt::find_sdhci() {
        let sdhci_base = sdhci_info.base as usize;
        let sdhci_irq = sdhci_info.irq;
        if sdhci::probe(sdhci_base) {
            crate::kinf!(
                "sdhci",
                "found at %p irq=%d",
                Arg::from(sdhci_base),
                Arg::from(sdhci_irq)
            );
            if sdhci::init(sdhci_base, sdhci_irq) {
                crate::kinf!("sdhci", "SD card initialized");
                module::register("sdhci", ModuleType::Driver);
            } else {
                crate::kwrn!("sdhci", "SD card init failed");
            }
        } else {
            crate::kinf!("sdhci", "probe failed at %p", Arg::from(sdhci_base));
        }
    }

    ndevs
}

pub(crate) unsafe fn probe_peripherals() {
    if let Some(rtc_info) = periph::find_rtc() {
        rtc::probe();
        crate::kinf!(
            "rtc",
            "base=%p src=%s",
            Arg::from(rtc_info.base),
            Arg::from(hwrand::source_name())
        );
    }
    unsafe {
        if usb::init_usb().is_ok() {
            crate::kinf!("usb", "host controller initialized");
        } else {
            crate::kwrn!("usb", "no host controller found");
        }
    }
    if let Some(gpio_info) = periph::find_gpio() {
        gpio::init(gpio_info.base as usize);
        crate::kinf!("gpio", "base=%p", Arg::from(gpio_info.base));
        let _ = led::bind(led::Led::Power, 5, true);
    }
    if let Some(i2c_info) = periph::find_i2c() {
        i2c::init(i2c_info.base as usize, 99);
        crate::kinf!("i2c", "base=%p", Arg::from(i2c_info.base));
    }
    if let Some(spi_info) = periph::find_spi() {
        spi::init(spi_info.base as usize, 4, 0);
        crate::kinf!("spi", "base=%p", Arg::from(spi_info.base));
    }
    if let Some(wdt_info) = periph::find_watchdog() {
        watchdog::init(wdt_info.base as usize);
        let _ = watchdog::arm(3000);
        crate::kinf!("wdt", "base=%p timeout=3000ms", Arg::from(wdt_info.base));
    }
    let extra_virtio_bases = [
        0x1000_1000usize,
        0x1000_2000,
        0x1000_3000,
        0x1000_4000,
        0x1000_5000,
        0x1000_6000,
        0x1000_7000,
        0x1000_8000,
    ];
    for &b in &extra_virtio_bases {
        if virtio_rng::probe(b) {
            if virtio_rng::init(b).is_ok() {
                crate::kinf!("virtio-rng", "init @ %p", Arg::from(b));
                module::register("virtio-rng", ModuleType::Driver);
            }
            continue;
        }
        if virtio_net::probe(b) {
            if virtio_net::init(b).is_ok() {
                let mac = virtio_net::mac();
                crate::kinf!(
                    "virtio-net",
                    "init @ %p mac=%x:%x:%x:%x:%x:%x",
                    Arg::from(b),
                    Arg::from(mac[0] as u32),
                    Arg::from(mac[1] as u32),
                    Arg::from(mac[2] as u32),
                    Arg::from(mac[3] as u32),
                    Arg::from(mac[4] as u32),
                    Arg::from(mac[5] as u32)
                );
                module::register("virtio-net", ModuleType::Driver);
            }
            continue;
        }
        if virtio_console::probe(b) {
            if virtio_console::init(b).is_ok() {
                crate::kinf!("virtio-console", "init @ %p", Arg::from(b));
                module::register("virtio-console", ModuleType::Driver);
            }
            continue;
        }
        if virtio_input::probe(b) {
            if virtio_input::init(b).is_ok() {
                crate::kinf!("virtio-input", "init @ %p", Arg::from(b));
                module::register("virtio-input", ModuleType::Driver);
            }
            continue;
        }
    }
    crate::kinf!("hwrand", "source=%s", Arg::from(hwrand::source_name()));
}

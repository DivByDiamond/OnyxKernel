use crate::arch::csr;
use crate::arch::regs::*;
use crate::drivers::{plic, sdhci, uart, virtio};
use crate::libfdt::fdt;
use crate::mm::{heap, pmm, vmm};
use crate::srv::{timer, trap};
use onyx_core::fmt::Arg;

pub(crate) unsafe fn early_init(fdt_addr: usize) {
    uart::init_default();

    if fdt::init(fdt_addr) {
        crate::kinf!("fdt", "parsed successfully");
    } else {
        crate::kwrn!("fdt", "parse failed, using defaults");
    }

    let mem = fdt::memory().unwrap_or(fdt::FdtMemory {
        base: 0x8000_0000,
        size: 0x1000_0000,
    });
    pmm::init(mem.base, mem.size);

    let _ = vmm::init();
    crate::kinf!("vmm", "Sv39 on, kernel root @%p", Arg::from(vmm::kernel_root()));

    heap::init();
    crate::kinf!("heap", "ready");

    trap::init();
    timer::init();

    if let Some(plic_base) = fdt::find_plic() {
        plic::init(plic_base);
        plic::set_priority(PLIC_PRIO_UART, 7);
        plic::set_priority(PLIC_PRIO_VIRTIO, 5);
        plic::enable(PLIC_PRIO_UART, 0);
        plic::set_threshold(0);
        csr::set_sie((1 << 1) | (1 << 9));
        crate::kinf!("plic", "base=%p", Arg::from(plic_base));
    }
}

pub(crate) unsafe fn probe_devices() -> usize {
    let mut ndevs = 0;
    let mut virtio_devs = [fdt::FdtMmio { base: 0, irq: 0, reg_shift: 0 }; 8];
    let nfound = fdt::find_virtio(&mut virtio_devs, 8);
    for dev in virtio_devs.iter().take(nfound) {
        let b = dev.base as usize;
        if b != 0 && virtio::probe(b) && virtio::init(b).is_ok() {
            ndevs += 1;
        }
    }
    if nfound == 0 {
        let bases = [0x1000_1000usize, 0x1000_2000, 0x1000_3000, 0x1000_4000,
                     0x1000_5000, 0x1000_6000, 0x1000_7000, 0x1000_8000];
        for &b in &bases {
            if virtio::probe(b) && virtio::init(b).is_ok() {
                ndevs += 1;
            }
        }
    }
    crate::kinf!("virtio-blk", "%d device(s)", Arg::from(ndevs));

    if let Some(sdhci_info) = fdt::find_sdhci() {
        let sdhci_base = sdhci_info.base as usize;
        let sdhci_irq = sdhci_info.irq;
        if sdhci::probe(sdhci_base) {
            crate::kinf!("sdhci", "found at %p irq=%d", Arg::from(sdhci_base), Arg::from(sdhci_irq));
            if sdhci::init(sdhci_base, sdhci_irq) {
                crate::kinf!("sdhci", "SD card initialized");
            } else {
                crate::kwrn!("sdhci", "SD card init failed");
            }
        } else {
            crate::kinf!("sdhci", "probe failed at %p", Arg::from(sdhci_base));
        }
    }

    ndevs
}

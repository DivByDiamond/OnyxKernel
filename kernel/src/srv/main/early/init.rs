use crate::arch::csr;
use crate::arch::regs::*;
use crate::drivers::{plic, uart};
use crate::libfdt::fdt;
use crate::mm::{heap, pmm, vmm};
use crate::module::{self, ModuleType};
use crate::srv::{timer, trap};
use onyx_core::fmt::Arg;

pub(crate) unsafe fn early_init(fdt_addr: usize) {
    // Console address comes from the device tree. On QEMU-virt it is the
    // legacy 0x10000000, but on OC2R/sedna the UART may be allocated at a
    // different address (devices are placed sequentially from 0x10000000).
    if fdt::init(fdt_addr) {
        if let Some(u) = fdt::find_uart() {
            uart::init(u.base as usize, u.reg_shift);
        } else {
            uart::init_default();
        }
        crate::kinf!("fdt", "parsed successfully");
    } else {
        uart::init_default();
        crate::kwrn!("fdt", "parse failed, using defaults");
    }
    module::register("uart", ModuleType::Driver);

    let mem = fdt::memory().unwrap_or(fdt::FdtMemory {
        base: 0x8000_0000,
        size: 0x1000_0000,
    });
    pmm::init(mem.base, mem.size);

    let _ = vmm::init();
    crate::kinf!(
        "vmm",
        "Sv39 on, kernel root @%p",
        Arg::from(vmm::kernel_root())
    );

    heap::init();
    crate::kinf!("heap", "ready");

    crate::proc::scheduler::runqueue::init();

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

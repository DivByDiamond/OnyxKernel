use onyx_core::fmt::Arg;

mod early;
mod display;
mod vfs;
mod init;

const BANNER: &str = "\n\x1b[32m░█▀█░█▀█░█░█░█░█\n░█░█░█░█░░█░░▄▀▄\n░▀▀▀░▀░▀░░▀░░▀░▀\x1b[0m\n  OnyxKernel v0.3 (Rust) — RISC-V 64 GC\n\n";

pub unsafe fn kmain(hartid: usize, fdt_addr: usize) -> ! {
    crate::srv::klog::puts(BANNER);
    crate::kinf!("kmain", "hartid=%d fdt=%p", Arg::from(hartid), Arg::from(fdt_addr));

    early::early_init(fdt_addr);
    let ndevs = early::probe_devices();
    early::probe_peripherals();
    crate::net::init(
        [10, 0, 2, 15],   // IP address (QEMU user-net default)
        [10, 0, 2, 2],    // Gateway
        [255, 255, 255, 0], // Netmask
    );
    crate::kinf!("net", "IP=%d.%d.%d.%d MAC=%x:%x:%x:%x:%x:%x",
        Arg::from(10), Arg::from(0),
        Arg::from(2), Arg::from(15),
        Arg::from(crate::drivers::virtio_net::mac()[0] as u32),
        Arg::from(crate::drivers::virtio_net::mac()[1] as u32),
        Arg::from(crate::drivers::virtio_net::mac()[2] as u32),
        Arg::from(crate::drivers::virtio_net::mac()[3] as u32),
        Arg::from(crate::drivers::virtio_net::mac()[4] as u32),
        Arg::from(crate::drivers::virtio_net::mac()[5] as u32));
    display::init_and_draw();
    vfs::setup(ndevs);
    vfs::load_font();
    init::launch()
}

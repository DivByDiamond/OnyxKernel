use onyx_core::fmt::Arg;

mod display;
mod early;
mod init;
mod vfs;

const BANNER: &str = "\n\x1b[32m░█▀█░█▀█░█░█░█░█\n░█░█░█░█░░█░░▄▀▄\n░▀▀▀░▀░▀░░▀░░▀░▀\x1b[0m\n  OnyxKernel v0.3 (Rust) — RISC-V 64 GC\n\n";

pub unsafe fn kmain(hartid: usize, fdt_addr: usize) -> ! {
    // Configure the console from the device tree before printing anything:
    // on OC2R/sedna the UART is not necessarily at the QEMU-virt default
    // 0x10000000, and a wrong console address makes the kernel appear dead.
    if crate::libfdt::fdt::init(fdt_addr) {
        if let Some(u) = crate::libfdt::fdt::find_uart() {
            crate::drivers::uart::init(u.base as usize, u.reg_shift);
        } else {
            crate::drivers::uart::init_default();
        }
    } else {
        crate::drivers::uart::init_default();
    }
    crate::srv::klog::puts(BANNER);
    crate::kinf!(
        "kmain",
        "hartid=%d fdt=%p",
        Arg::from(hartid),
        Arg::from(fdt_addr)
    );

    early::early_init(fdt_addr);
    let ndevs = early::probe_devices();
    early::probe_peripherals();
    // Network config: try DHCP first (OC2R/sedna net), fall back to the
    // QEMU user-net defaults. dhcp_discover returns (ip, mask, gateway, dns).
    let (ip, gw, mask, dns);
    if let Ok((d_ip, d_mask, d_gw, d_dns)) = crate::net::dhcp::dhcp_discover() {
        ip = d_ip;
        gw = d_gw;
        mask = d_mask;
        dns = d_dns;
        crate::kinf!("net", "DHCP lease acquired");
    } else {
        ip = [10, 0, 2, 15];
        gw = [10, 0, 2, 2];
        mask = [255, 255, 255, 0];
        dns = [10, 0, 2, 3];
        crate::kwrn!("net", "DHCP failed, using QEMU user-net defaults");
    }
    crate::net::init(ip, gw, mask);
    crate::net::G_DNS = dns;
    crate::kinf!(
        "net",
        "IP=%d.%d.%d.%d gw=%d.%d.%d.%d mask=%d.%d.%d.%d MAC=%x:%x:%x:%x:%x:%x",
        Arg::from(ip[0] as u32),
        Arg::from(ip[1] as u32),
        Arg::from(ip[2] as u32),
        Arg::from(ip[3] as u32),
        Arg::from(gw[0] as u32),
        Arg::from(gw[1] as u32),
        Arg::from(gw[2] as u32),
        Arg::from(gw[3] as u32),
        Arg::from(mask[0] as u32),
        Arg::from(mask[1] as u32),
        Arg::from(mask[2] as u32),
        Arg::from(mask[3] as u32),
        Arg::from(crate::drivers::virtio_net::mac()[0] as u32),
        Arg::from(crate::drivers::virtio_net::mac()[1] as u32),
        Arg::from(crate::drivers::virtio_net::mac()[2] as u32),
        Arg::from(crate::drivers::virtio_net::mac()[3] as u32),
        Arg::from(crate::drivers::virtio_net::mac()[4] as u32),
        Arg::from(crate::drivers::virtio_net::mac()[5] as u32)
    );
    display::init_and_draw();
    vfs::setup(ndevs);
    vfs::load_font();
    init::launch()
}

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
    display::init_and_draw();
    vfs::setup(ndevs);
    vfs::load_font();
    init::launch()
}

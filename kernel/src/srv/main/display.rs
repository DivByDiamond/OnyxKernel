use crate::drivers::fb;
use crate::mm::pmm;
use onyx_core::fmt::Arg;

pub(crate) unsafe fn init_and_draw() {
    // Framebuffer always from pmm. The ECAM PCI scan (hardcoded QEMU-virt
    // 0x30000000) is meaningless on OC2R/sedna and has been observed to
    // return a bogus display BAR (0x10100000) whose region is not a real
    // device — clearing it page-faults. Kernel console is the UART anyway.
    let fb_pages = (fb::FB_SIZE + 4095) / 4096;
    let fb_pa = pmm::alloc_n(fb_pages).ok().map(|pa| {
        crate::kinf!("fb", "allocated at %p", Arg::from(pa));
        pa as usize
    });
    if let Some(pa) = fb_pa {
        if fb::init(pa).is_ok() {
            crate::kinf!("fb", "init ok");
        } else {
            crate::kwrn!("fb", "init failed");
        }
    }
    if fb::enabled() {
        fb::clear();
        let banner = "\n░█▀█░█▀█░█░█░█░█\n░█░█░█░█░░█░░▄▀▄\n░▀▀▀░▀░▀░░▀░░▀░▀\n  OnyxKernel v0.3 (Rust) — RISC-V 64 GC\n\n";
        let mut y = 40usize;
        for line in banner.lines() {
            let x = (fb::FB_WIDTH - line.len() * 8) / 2;
            fb::draw_str(x, y, line, 0x00FF00, 0x000000);
            y += 16;
        }
        fb::draw_str(10, y + 8, "Booting...", 0x00AAAA, 0x000000);
    }
}

use crate::drivers::fb;
use crate::mm::pmm;
use onyx_core::fmt::Arg;

pub(crate) unsafe fn init_and_draw() {
    unsafe {
        // Prefer the device-tree simple-framebuffer (OC2R/sedna monitor): it is an
        // MMIO region the host actually samples, whereas private RAM is invisible
        // outside the VM. Fallback: pmm RAM (QEMU without virtio-gpu). The ECAM PCI
        // scan (hardcoded QEMU-virt 0x30000000) is meaningless on OC2R/sedna and has
        // been observed to return a bogus display BAR — never use it here.
        let fdt_fb = crate::libfdt::fdt::find_simple_framebuffer();
        if let Some(sfb) = fdt_fb {
            let stride = if sfb.stride != 0 {
                sfb.stride as usize
            } else {
                sfb.width as usize * 2
            };
            if fb::init_device(sfb.pa, sfb.width as usize, sfb.height as usize, stride, 16).is_ok()
            {
                crate::kinf!("fb", "simple-framebuffer at %p", Arg::from(sfb.pa));
            } else {
                crate::kwrn!("fb", "simple-framebuffer init failed");
            }
        }
        if !fb::enabled() {
            let fb_pages = fb::size_bytes().div_ceil(4096);
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
        }
        draw_banner();
    }
}

fn draw_banner() {
    if fb::enabled() {
        fb::clear();
        let banner = "\n░█▀█░█▀█░█░█░█░█\n░█░█░█░█░░█░░▄▀▄\n░▀▀▀░▀░▀░░▀░░▀░▀\n  OnyxKernel v0.3 (Rust) — RISC-V 64 GC\n\n";
        let mut y = 40usize;
        for line in banner.lines() {
            let x = (fb::width().saturating_sub(line.len() * 8)) / 2;
            fb::draw_str(x, y, line, 0x00FF00, 0x000000);
            y += 16;
        }
        fb::draw_str(10, y + 8, "Booting...", 0x00AAAA, 0x000000);
    }
}

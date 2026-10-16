use crate::drivers::video::ramfb;
use crate::drivers::{fb, virtio_gpu};
use crate::mm::pmm;
use onyx_core::fmt::Arg;

/// # Safety
///
/// Boot-time framebuffer init: must run single-threaded on the boot hart
/// after pmm/heap are up (it may allocate framebuffer pages); draws the
/// banner exactly once.
pub(crate) unsafe fn init_and_draw() {
    // SAFETY: framebuffer memory is the DTB simple-framebuffer region or fresh pmm pages; single-threaded boot call.
    unsafe {
        // Prefer the device-tree simple-framebuffer (OC2R/sedna monitor): it is an
        // MMIO region the host actually samples, whereas private RAM is invisible
        // outside the VM. Fallback: pmm RAM (QEMU without virtio-gpu). The ECAM PCI
        // scan (hardcoded QEMU-virt 0x30000000) is meaningless on OC2R/sedna and has
        // been observed to return a bogus display BAR — never use it here.
        let mut displays = [crate::libfdt::fdt::SimpleFramebuffer::default(); fb::devices::MAX_FRAMEBUFFERS];
        let display_count = crate::libfdt::fdt::find_simple_framebuffers(&mut displays);
        for display in &displays[..display_count] {
            let result = if !fb::enabled() {
                fb::init_device(display.pa, display.width as usize, display.height as usize,
                    display.stride as usize, display.bpp as usize)
            } else {
                fb::Framebuffer::new(display.pa, display.width as usize, display.height as usize,
                    display.stride as usize, display.bpp as usize)
                    .and_then(|surface| fb::devices::register(surface).map(|_| ()))
            };
            if result.is_ok() {
                crate::kinf!("fb", "simple-framebuffer at %p", Arg::from(display.pa));
            } else {
                crate::kwrn!("fb", "simple-framebuffer init failed");
            }
        }
        if !fb::enabled() {
            // Try virtio-gpu (QEMU virt -device virtio-gpu-device) before falling
            // back to invisible RAM. The virtio-mmio bases are the fixed QEMU
            // virt constants; the GPU is probed synchronously and its framebuffer
            // becomes the visible front buffer if init succeeds.
            //
            // Bug fix (OC2R issue #41, 2026-09-15): this loop unconditionally
            // probed the hardcoded QEMU-virt bases even on OC2R/sedna, which has
            // no device mapped at 0x1000_1000 -- confirmed by a live crash trace
            // (scause=5 load access fault, stval=0x10001000, matching the first
            // entry of `virtio_bases` exactly) with debug marks showing the fault
            // happens before the PCI VGA fallback even runs. `early::probe_devices`
            // already gates this same hardcoded-base fallback behind
            // `!fdt::is_sedna()` (see probe.rs) for its virtio-blk scan; this loop
            // was the one place that pattern was missed. sedna's virtio devices
            // are always FDT-discovered (probe_devices already tried them above
            // via `find_virtio`), so skipping the hardcoded scan here loses
            // nothing on sedna.
            let virtio_bases = [
                0x1000_1000usize,
                0x1000_2000,
                0x1000_3000,
                0x1000_4000,
                0x1000_5000,
                0x1000_6000,
                0x1000_7000,
                0x1000_8000,
            ];
            let mut tried_gpu = false;
            let sedna = crate::libfdt::fdt::is_sedna();
            for &b in if sedna { &[][..] } else { &virtio_bases[..] } {
                if virtio_gpu::probe(b) {
                    tried_gpu = true;
                    crate::kinf!("display", "virtio-gpu probe hit base=%p", Arg::from(b));
                    match virtio_gpu::init(b, 1280, 720) {
                        Ok(()) => {
                            let fb_pa = virtio_gpu::fb_addr() as usize;
                            if fb_pa != 0 {
                                if fb::init(fb_pa).is_ok() {
                                    crate::kinf!("display", "virtio-gpu at %p", Arg::from(b));
                                } else {
                                    crate::kwrn!(
                                        "display",
                                        "virtio-gpu fb init failed at %p",
                                        Arg::from(b)
                                    );
                                }
                            } else {
                                crate::kwrn!(
                                    "display",
                                    "virtio-gpu init ok but fb=0 at %p",
                                    Arg::from(b)
                                );
                            }
                        }
                        Err(e) => {
                            crate::kwrn!(
                                "display",
                                "virtio-gpu init failed at %p err=%d",
                                Arg::from(b),
                                Arg::from(e.as_i64())
                            );
                        }
                    }
                    break;
                }
            }
            if !fb::enabled() && !sedna {
                // ramfb (QEMU -device ramfb) as second fallback — no virtio
                // queue, just a fw_cfg write. Works with -display gtk and
                // requires no vgabios.
                //
                // Bug fix (OC2R issue #41, 2026-09-15): confirmed faulting on
                // sedna by a live crash trace (scause=7 store/AMO access fault,
                // stval=0x10100008, exactly FW_CFG_SELECTOR = FW_CFG_BASE+8) hit
                // immediately after the virtio-gpu loop above was fixed to skip
                // sedna. Same class of bug: fw_cfg @ 0x1010_0000 is a
                // QEMU-virt-fixed address sedna doesn't back. Gated the same way.
                crate::drivers::uart::putc(b'(');
                let r = ramfb::init(1280, 720);
                crate::drivers::uart::putc(b')');
                if let Ok(pa) = r
                    && fb::init(pa).is_ok()
                {
                    crate::kinf!("display", "ramfb at %p", Arg::from(pa));
                }
            }
            if !fb::enabled() && !sedna {
                // PCI VGA (bochs-display) as third fallback for QEMU virt.
                // Bug fix (OC2R issue #41, 2026-09-15): same class of bug as the
                // virtio-gpu and ramfb fallbacks above (now confirmed twice by live
                // crash traces) -- ECAM_BASE=0x3000_0000 is another QEMU-virt-fixed
                // address sedna doesn't back. Gated the same runtime way; debug
                // marks kept below in case a real PCI bus build (non-sedna) still
                // needs tracing here.
                crate::drivers::uart::putc(b'{');
                let scan = crate::drivers::bus::pci::find_vga_fb();
                crate::drivers::uart::putc(b'}');
                if let Ok(vga_pa) = scan {
                    crate::kinf!("display", "pci VGA scan returned %p", Arg::from(vga_pa as u64));
                    if vga_pa >= 0x4000_0000 {
                        crate::drivers::uart::putc(b'[');
                        let ok = fb::init_device(vga_pa, 1280, 720, 1280 * 4, 32).is_ok();
                        crate::drivers::uart::putc(b']');
                        if ok {
                            crate::kinf!("display", "pci VGA at %p", Arg::from(vga_pa));
                        }
                    } else {
                        crate::kwrn!(
                            "display",
                            "pci VGA scan returned sub-4GiB addr %p, rejected",
                            Arg::from(vga_pa as u64)
                        );
                    }
                } else {
                    crate::kwrn!("display", "pci VGA scan found nothing");
                }
            }
            if !fb::enabled() {
                if tried_gpu {
                    crate::kwrn!(
                        "display",
                        "virtio-gpu+ramfb+PCI VGA failed, falling back to RAM"
                    );
                }
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
        // The banner paints into the console BACK buffer (double-buffered
        // anti-flicker path); present it once so it is visible immediately
        // instead of waiting for the first debounced console present.
        fb::present();
    }
}

use crate::drivers::fb;
use crate::drivers::virtio_gpu;
use crate::mm::pmm;
use core::ptr;
use onyx_core::errno::{Errno, KResult};
use onyx_core::fmt::Arg;

pub struct DisplayMode {
    pub width: u16,
    pub height: u16,
    pub refresh: u8,
    pub bpp: u8,
    pub name: &'static str,
}

pub struct DisplayState {
    mode: DisplayMode,
    fb_base: *mut u8,
    fb_size: usize,
    enabled: bool,
}

pub static mut G_DISPLAY: DisplayState = DisplayState {
    mode: DisplayMode {
        width: 1280,
        height: 720,
        refresh: 60,
        bpp: 32,
        name: "1280x720",
    },
    fb_base: ptr::null_mut(),
    fb_size: 0,
    enabled: false,
};

pub const MODES: &[DisplayMode] = &[
    DisplayMode {
        width: 640,
        height: 480,
        refresh: 60,
        bpp: 32,
        name: "640x480",
    },
    DisplayMode {
        width: 800,
        height: 600,
        refresh: 60,
        bpp: 32,
        name: "800x600",
    },
    DisplayMode {
        width: 1024,
        height: 768,
        refresh: 60,
        bpp: 32,
        name: "1024x768",
    },
    DisplayMode {
        width: 1280,
        height: 720,
        refresh: 60,
        bpp: 32,
        name: "1280x720",
    },
    DisplayMode {
        width: 1920,
        height: 1080,
        refresh: 60,
        bpp: 32,
        name: "1920x1080",
    },
];

/// # Safety
///
/// Stub: performs no unsafe operations; the unsafe marker is reserved for a
/// future device-probing implementation.
pub unsafe fn probe_all() -> KResult<()> {
    Err(Errno::NoEnt)
}

/// # Safety
///
/// Must run during single-threaded display setup (SIE=0) since it mutates
/// `G_DISPLAY`; `mode` is trusted driver-side geometry.
pub unsafe fn set_mode(mode: &DisplayMode) -> KResult<()> {
    // SAFETY: G_DISPLAY is only mutated here during single-threaded setup (SIE=0, see crate::sync); fb_pa comes from pmm::alloc_n and fb::init validates the address.
    unsafe {
        if !G_DISPLAY.enabled {
            return Err(Errno::NoSys);
        }
        let w = mode.width as u32;
        let h = mode.height as u32;
        let bpp = mode.bpp as u32;
        let fb_size = w as usize * h as usize * (bpp as usize / 8);
        let fb_pages = fb_size.div_ceil(4096);
        let fb_pa = pmm::alloc_n(fb_pages)?;
        G_DISPLAY.fb_base = fb_pa as *mut u8;
        G_DISPLAY.fb_size = fb_size;
        G_DISPLAY.mode = DisplayMode {
            width: mode.width,
            height: mode.height,
            refresh: mode.refresh,
            bpp: mode.bpp,
            name: mode.name,
        };
        fb::init(fb_pa as usize).ok();
        if virtio_gpu::probe(virtio_gpu::G_GPU.base) {
            virtio_gpu::init(virtio_gpu::G_GPU.base, w, h).ok();
        }
        Ok(())
    }
}

/// # Safety
///
/// Must run once during single-threaded boot init (SIE=0); `_fdt_addr` is
/// currently unused and probing uses the fixed platform MMIO bases below.
pub unsafe fn init_display(_fdt_addr: usize) -> KResult<()> {
    // SAFETY: G_DISPLAY is written once here during single-threaded boot (SIE=0); virtio bases are the fixed platform MMIO constants in this function, the fallback fb_pa comes from pmm::alloc_n.
    unsafe {
        let mut found_virtio_gpu = false;
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
        for &b in &virtio_bases {
            if virtio_gpu::probe(b) {
                if virtio_gpu::init(b, 1280, 720).is_ok() {
                    let fb_pa = virtio_gpu::fb_addr() as usize;
                    if fb_pa != 0 {
                        fb::init(fb_pa).ok();
                        G_DISPLAY.fb_base = virtio_gpu::fb_addr();
                        G_DISPLAY.fb_size = virtio_gpu::fb_size();
                        G_DISPLAY.enabled = true;
                        found_virtio_gpu = true;
                        crate::kinf!("display", "virtio-gpu at %p", Arg::from(b));
                    }
                }
                break;
            }
        }
        if !found_virtio_gpu {
            let fb_pages = fb::FB_SIZE.div_ceil(4096);
            if let Ok(pa) = pmm::alloc_n(fb_pages) {
                fb::init(pa as usize).ok();
                G_DISPLAY.fb_base = pa as *mut u8;
                G_DISPLAY.fb_size = fb::FB_SIZE;
                G_DISPLAY.enabled = true;
                crate::kinf!("display", "fallback fb at %p", Arg::from(pa));
            } else {
                return Err(Errno::NoMem);
            }
        }
        Ok(())
    }
}

pub fn current_mode() -> &'static DisplayMode {
    // SAFETY: read-only borrow of G_DISPLAY.mode, written only during single-threaded init (SIE=0, see crate::sync).
    unsafe { &G_DISPLAY.mode }
}

use onyx_core::errno::KResult;

pub const FB_WIDTH: usize = 1280;
pub const FB_HEIGHT: usize = 720;
pub const FB_BPP: usize = 32;
pub const FB_PITCH: usize = FB_WIDTH * (FB_BPP / 8);
pub const FB_SIZE: usize = FB_HEIGHT * FB_PITCH;
pub(crate) const COL_BLACK: u32 = 0x000000;
pub(crate) const COL_GREEN: u32 = 0x00FF00;
pub(crate) const COL_RED: u32 = 0xFF0000;
pub(crate) const COL_YELLOW: u32 = 0xFFFF00;
pub(crate) const COL_BLUE: u32 = 0x0000FF;
pub(crate) const COL_MAGENTA: u32 = 0xFF00FF;
pub(crate) const COL_CYAN: u32 = 0x00FFFF;
pub(crate) const COL_WHITE: u32 = 0xFFFFFF;

/// (width, pitch, bpp, height, base) for ANSI scroll/erase fast paths.
pub fn info() -> (usize, usize, usize, usize, usize) {
    unsafe {
        (
            G_FB.width,
            G_FB.pitch,
            G_FB.bpp,
            G_FB.height,
            G_FB.base as usize,
        )
    }
}

/// Blend-safe pixel write used by the ANSI eraser (handles 16/32 bpp).
pub fn put_pixel_blend(x: usize, y: usize, color: u32) {
    put_pixel(x, y, color);
}

static mut G_FB: Fb = Fb {
    base: core::ptr::null_mut(),
    width: FB_WIDTH,
    height: FB_HEIGHT,
    pitch: FB_PITCH,
    bpp: FB_BPP,
    enabled: false,
};

#[derive(Clone, Copy)]
pub struct Fb {
    base: *mut u8,
    width: usize,
    height: usize,
    pitch: usize,
    bpp: usize,
    enabled: bool,
}

pub fn enabled() -> bool {
    unsafe { G_FB.enabled }
}

pub fn width() -> usize {
    unsafe { G_FB.width }
}

pub fn height() -> usize {
    unsafe { G_FB.height }
}

pub fn bpp() -> usize {
    unsafe { G_FB.bpp }
}

pub fn pitch() -> usize {
    unsafe { G_FB.pitch }
}

/// Total framebuffer size in bytes for the *current* mode.
pub fn size_bytes() -> usize {
    unsafe { G_FB.pitch * G_FB.height }
}

pub fn fb_base_ptr() -> *mut u8 {
    unsafe { G_FB.base }
}

pub fn fb_base_pa() -> usize {
    unsafe { G_FB.base as usize }
}

pub unsafe fn init(paddr: usize) -> KResult<()> {
    // Only accept pmm-managed RAM. On OC2R the ECAM PCI scan can report a
    // bogus display BAR in device space (e.g. 0x10100000) that is not backed
    // by a real device — clearing it page-faults the kernel.
    if paddr < 0x8000_0000 {
        return Err(onyx_core::errno::Errno::Inval);
    }
    init_device(paddr, FB_WIDTH, FB_HEIGHT, FB_PITCH, FB_BPP)
}

/// Init from a device-provided framebuffer (FDT `simple-framebuffer` on
/// OC2R/sedna): the address is MMIO outside pmm-managed RAM, and the geometry
/// comes from the device tree node, so both differ from the defaults above.
pub unsafe fn init_device(
    paddr: usize,
    width: usize,
    height: usize,
    stride: usize,
    bpp: usize,
) -> KResult<()> {
    if paddr == 0 || width == 0 || height == 0 || bpp == 0 || stride < width * (bpp / 8) {
        return Err(onyx_core::errno::Errno::Inval);
    }
    G_FB = Fb {
        base: paddr as *mut u8,
        width,
        height,
        pitch: stride,
        bpp,
        enabled: true,
    };
    clear();
    Ok(())
}

pub fn clear() {
    unsafe {
        if !G_FB.enabled {
            return;
        }
        let base = G_FB.base;
        let size = G_FB.pitch * G_FB.height;
        for i in 0..size {
            *base.add(i) = 0;
        }
    }
}

fn put_pixel(x: usize, y: usize, color: u32) {
    unsafe {
        if !G_FB.enabled || x >= G_FB.width || y >= G_FB.height {
            return;
        }
        let off = y * G_FB.pitch + x * (G_FB.bpp / 8);
        let base = G_FB.base;
        if G_FB.bpp <= 16 {
            // RGB565 (r5g6b5), little-endian — the OC2R monitor format.
            let r5 = ((color >> 16) & 0xF8) as u16;
            let g6 = ((color >> 8) & 0xFC) as u16;
            let b5 = (color & 0xF8) as u16;
            let px: u16 = r5 << 11 | g6 << 5 | b5;
            core::ptr::write_volatile(base.add(off) as *mut u16, px.to_le());
        } else {
            *base.add(off) = (color & 0xFF) as u8;
            *base.add(off + 1) = ((color >> 8) & 0xFF) as u8;
            *base.add(off + 2) = ((color >> 16) & 0xFF) as u8;
        }
    }
}

pub mod draw;
pub mod scroll;
pub use draw::*;
pub use scroll::*;

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
    // SAFETY: read-only snapshot of G_FB fields, installed once at single-threaded init (SIE=0).
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
/// Targets the console paint surface (back buffer when double buffering
/// is active, else the front buffer).
pub fn put_pixel_blend(x: usize, y: usize, color: u32) {
    put_pixel(x, y, color);
}

/// Front-buffer base (kernel direct-mapped VA of the visible surface).
#[inline]
pub fn front_base() -> *mut u8 {
    // SAFETY: plain read of the G_FB.base pointer, set once at single-threaded init (SIE=0).
    unsafe { G_FB.base }
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
    // SAFETY: plain read of the G_FB.enabled flag; no concurrent mutation (SIE=0, see crate::sync).
    unsafe { G_FB.enabled }
}

pub fn width() -> usize {
    // SAFETY: plain read of the G_FB.width field, set once at single-threaded init (SIE=0).
    unsafe { G_FB.width }
}

pub fn height() -> usize {
    // SAFETY: plain read of the G_FB.height field, set once at single-threaded init (SIE=0).
    unsafe { G_FB.height }
}

pub fn bpp() -> usize {
    // SAFETY: plain read of the G_FB.bpp field, set once at single-threaded init (SIE=0).
    unsafe { G_FB.bpp }
}

pub fn pitch() -> usize {
    // SAFETY: plain read of the G_FB.pitch field, set once at single-threaded init (SIE=0).
    unsafe { G_FB.pitch }
}

/// Total framebuffer size in bytes for the *current* mode.
pub fn size_bytes() -> usize {
    // SAFETY: arithmetic on G_FB fields set once at single-threaded init (SIE=0).
    unsafe { G_FB.pitch * G_FB.height }
}

pub fn fb_base_ptr() -> *mut u8 {
    // SAFETY: plain read of the G_FB.base pointer, set once at single-threaded init (SIE=0).
    unsafe { G_FB.base }
}

pub fn fb_base_pa() -> usize {
    // SAFETY: plain read of the G_FB.base pointer, set once at single-threaded init (SIE=0).
    unsafe { G_FB.base as usize }
}

/// # Safety
///
/// `paddr` must be a usable framebuffer base: pmm-managed RAM (range-checked
/// here) or a device framebuffer from the FDT `simple-framebuffer` node;
/// must run during single-threaded boot init (SIE=0).
pub unsafe fn init(paddr: usize) -> KResult<()> {
    // SAFETY: paddr is range-checked against pmm-managed RAM before delegation; G_FB is only installed via init_device during single-threaded init.
    unsafe {
        // Only accept pmm-managed RAM. On OC2R the ECAM PCI scan can report a
        // bogus display BAR in device space (e.g. 0x10100000) that is not backed
        // by a real device — clearing it page-faults the kernel.
        if paddr < 0x8000_0000 {
            return Err(onyx_core::errno::Errno::Inval);
        }
        init_device(paddr, FB_WIDTH, FB_HEIGHT, FB_PITCH, FB_BPP)
    }
}

/// Init from a device-provided framebuffer (FDT `simple-framebuffer` on
/// OC2R/sedna): the address is MMIO outside pmm-managed RAM, and the geometry
/// comes from the device tree node, so both differ from the defaults above.
///
/// # Safety
///
/// `paddr` must be the real, identity-mapped framebuffer base reported by
/// the device (FDT `simple-framebuffer` node); geometry is validated below
/// before `G_FB` is installed.
pub unsafe fn init_device(
    paddr: usize,
    width: usize,
    height: usize,
    stride: usize,
    bpp: usize,
) -> KResult<()> {
    // SAFETY: geometry (non-zero dims, stride >= width*bpp/8) is validated above; G_FB is written once during single-threaded boot init (SIE=0, see crate::sync).
    unsafe {
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
        // Anti-flicker: allocate the console back buffer for this geometry
        // right away (single-threaded init context, pmm is up and fresh).
        // On failure the console keeps legacy direct-to-front painting.
        draw::init_back_buffer();
        // Geometry is now established/changed (todo P2 #1): flag the resize
        // so the first TIOCGWINSZ after this delivers SIGWINCH. At boot
        // there is no foreground process, so the direct signal is a no-op.
        resize::note_resized();
        Ok(())
    }
}

pub fn clear() {
    // SAFETY: base/size come from the validated G_FB installed at init; vzero stays within pitch*height bytes of the mapped framebuffer.
    unsafe {
        if !G_FB.enabled {
            return;
        }
        let base = G_FB.base;
        let size = G_FB.pitch * G_FB.height;
        scroll::vzero(base, size);
    }
}

/// Pixel write onto the console paint surface (back buffer when double
/// buffering is active, else the visible front buffer). The paint base
/// itself is [`draw::paint_base`], re-exported as `fb::paint_base`.
fn put_pixel(x: usize, y: usize, color: u32) {
    // Bounds/format checks plus the volatile stores live in
    // put_pixel_at_base (front and back share the validated geometry).
    put_pixel_at_base(draw::paint_base(), x, y, color);
}

/// Pixel write onto the VISIBLE front buffer. Used only by the present-time
/// cursor block (drawn after the back buffer is copied over the front).
pub(crate) fn put_pixel_front(x: usize, y: usize, color: u32) {
    // Bounds/format checks plus the volatile stores live in
    // put_pixel_at_base; this targets the visible front surface only.
    put_pixel_at_base(front_base(), x, y, color);
}

/// Raw pixel write to an explicit surface base (front or back; both share
/// the validated G_FB geometry and pixel format).
///
/// # Safety contract (enforced by callers)
/// `base` must be the front framebuffer base or the published back-buffer
/// base, both of `pitch * height` bytes with the installed bpp.
fn put_pixel_at_base(base: *mut u8, x: usize, y: usize, color: u32) {
    // SAFETY: x/y are bounds-checked against width/height and off is derived from the validated pitch/bpp, so each volatile store stays inside the surface set up at init.
    unsafe {
        if !G_FB.enabled || x >= G_FB.width || y >= G_FB.height {
            return;
        }
        let off = y * G_FB.pitch + x * (G_FB.bpp / 8);
        if G_FB.bpp <= 16 {
            // RGB565 (r5g6b5), little-endian — the OC2R monitor format.
            let px = rgb32_to_r5g6b5(color);
            core::ptr::write_volatile(base.add(off) as *mut u16, px.to_le());
        } else if G_FB.bpp >= 32 {
            // X8R8G8B8, little-endian: blue in the low byte, matching the
            // byte-wise order this replaces.
            let px = color & 0xFF_FFFF;
            core::ptr::write_volatile(base.add(off) as *mut u32, px.to_le());
        } else {
            *base.add(off) = (color & 0xFF) as u8;
            *base.add(off + 1) = ((color >> 8) & 0xFF) as u8;
            *base.add(off + 2) = ((color >> 16) & 0xFF) as u8;
        }
    }
}

pub mod draw;
pub mod resize;
pub mod scroll;
pub use draw::*;
pub use scroll::*;

// Console double buffering (back-buffer paint surface, debounced present,
// init_back_buffer/double_buffered/paint_base/present) lives in draw.rs;
// the lazy get_back_buffer/swap_buffers stubs were replaced by it.

/// Test-only hook: install a host-provided surface as the front buffer so
/// host integration tests can drive the real paint/present pipeline without
/// pmm/UART (both are uninitialized on the host test harness). Never built
/// into the kernel image (cfg(test) on the bin test target only).
#[cfg(test)]
pub unsafe fn test_install_front(base: *mut u8, w: usize, h: usize, stride: usize, bpp: usize) {
    // SAFETY: test-only; G_FB is otherwise written by init/init_device.
    unsafe {
        G_FB = Fb {
            base,
            width: w,
            height: h,
            pitch: stride,
            bpp,
            enabled: true,
        };
    }
}

/// Test-only hook: disable the framebuffer between host tests.
#[cfg(test)]
pub fn test_uninstall_front() {
    // SAFETY: test-only; puts the driver back into its pre-init state.
    unsafe {
        G_FB.enabled = false;
        G_FB.base = core::ptr::null_mut();
    }
}

/// Quantize an 8-8-8 RGB value to the OC2R monitor's r5g6b5 (RGB565)
/// little-endian pixel format: R 5 bits (bits 11-15), G 6 bits (bits 5-10),
/// B 5 bits (bits 0-4), low bits of each channel truncated.
///
/// Bug fix (todo P4, host test): the previous inline math shifted every
/// field by the wrong amount (red landed at bits 10-14, green at 7-12 and
/// blue stayed at 3-7), so on a 16bpp OC2R framebuffer the three channels
/// bled into each other. Verified by test_rgb565_* against the standard
/// 0xF800/0x07E0/0x001F field masks.
pub(crate) fn rgb32_to_r5g6b5(color: u32) -> u16 {
    (((color >> 16) & 0xF8) as u16) << 8
        | (((color >> 8) & 0xFC) as u16) << 3
        | ((color & 0xF8) as u16) >> 3
}

#[cfg(test)]
mod tests {
    use super::rgb32_to_r5g6b5 as conv;

    #[test]
    fn test_rgb565_primary_colors() {
        // Pure channels land on the top bits of their fields.
        assert_eq!(conv(0xFF0000), 0xF800);
        assert_eq!(conv(0x00FF00), 0x07E0);
        assert_eq!(conv(0x0000FF), 0x001F);
        assert_eq!(conv(0xFFFFFF), 0xFFFF);
        assert_eq!(conv(0x000000), 0x0000);
    }

    #[test]
    fn test_rgb565_channel_quantization() {
        // Low bits are truncated: R/B drop byte bits 0-2, G drops bits 0-1.
        assert_eq!(conv(0x040404), conv(0x070707));
        assert_eq!(conv(0x000004), 0x0000); // B bit 2 dropped
        assert_eq!(conv(0x000008), 0x0001); // B bit 3 = B-field LSB
        assert_eq!(conv(0x000200), 0x0000); // G bit 1 dropped
        assert_eq!(conv(0x000400), 0x0020); // G bit 2 = G-field LSB
    }
}

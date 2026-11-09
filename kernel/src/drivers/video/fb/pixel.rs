//! Raw pixel write (front/back surface) and RGB565 quantization.
use super::{G_FB, front_base};

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

/// Pixel write onto the console paint surface (back buffer when double
/// buffering is active, else the visible front buffer). The paint base
/// itself is [`super::draw::paint_base`], re-exported as `fb::paint_base`.
pub(super) fn put_pixel(x: usize, y: usize, color: u32) {
    // Bounds/format checks plus the volatile stores live in
    // put_pixel_at_base (front and back share the validated geometry).
    put_pixel_at_base(super::draw::paint_base(), x, y, color);
}

/// Pixel write onto the VISIBLE front buffer. Used only by the present-time
/// cursor block (drawn after the back buffer is copied over the front).
pub(crate) fn put_pixel_front(x: usize, y: usize, color: u32) {
    // Bounds/format checks plus the volatile stores live in
    // put_pixel_at_base; this targets the visible front surface only.
    put_pixel_at_base(front_base(), x, y, color);
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
mod tests;

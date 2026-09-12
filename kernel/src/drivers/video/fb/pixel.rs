//! Raw pixel write (front/back surface) and RGB565 quantization.
use super::{front_base, put_pixel_at_base};

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

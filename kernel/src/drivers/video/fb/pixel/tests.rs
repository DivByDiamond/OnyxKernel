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

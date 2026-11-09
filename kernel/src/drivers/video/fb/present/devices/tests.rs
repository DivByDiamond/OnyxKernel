use super::*;
#[test]
fn validates_surface_bounds_before_exposing_memory() {
    assert_eq!(
        Framebuffer::new(0x30000000, 640, 400, 1280, 16)
            .unwrap()
            .size,
        512000
    );
    assert!(Framebuffer::new(0, 640, 400, 1280, 16).is_err());
    assert!(Framebuffer::new(0x30000000, 640, 400, 1279, 16).is_err());
    assert!(Framebuffer::new(0x30000001, 640, 400, 1280, 16).is_err());
    assert!(Framebuffer::new(0x30000000, usize::MAX, 2, 1280, 32).is_err());
    assert!(Framebuffer::new(usize::MAX - 3, 2, 2, 4, 16).is_err());
}

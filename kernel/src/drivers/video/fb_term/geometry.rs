//! Geometry of the stdout stream mirrored to both UART and framebuffer.
use core::sync::atomic::{AtomicBool, Ordering};
static SERIAL_MIRROR: AtomicBool = AtomicBool::new(false);
pub fn set_serial_mirror(enabled: bool) {
    SERIAL_MIRROR.store(enabled, Ordering::Relaxed);
}
pub(crate) fn serial_mirrored() -> bool {
    SERIAL_MIRROR.load(Ordering::Relaxed)
}
pub fn console_geometry() -> (usize, usize) {
    geometry(
        super::fb::width(),
        super::fb::height(),
        SERIAL_MIRROR.load(Ordering::Relaxed),
    )
}
fn geometry(width: usize, height: usize, mirrored: bool) -> (usize, usize) {
    // Sedna mirrors one stdout byte stream to the fixed 80x24 PC UART
    // terminal and the primary framebuffer. TUI coordinates must agree
    // on both displays; framebuffer device geometry itself remains intact.
    if mirrored {
        ((height / 16).clamp(1, 24), (width / 8).clamp(1, 80))
    } else {
        ((height / 16).max(1), (width / 8).max(1))
    }
}
#[cfg(test)]
mod tests {
    use super::geometry;
    #[test]
    fn mirrored_stdout_matches_oc2r_uart() {
        assert_eq!(geometry(1920, 1080, true), (24, 80));
        assert_eq!(geometry(640, 400, true), (24, 80));
        assert_eq!(geometry(320, 200, true), (12, 40));
    }
    #[test]
    fn standalone_framebuffer_retains_full_size() {
        assert_eq!(geometry(1920, 1080, false), (67, 240));
    }
}

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

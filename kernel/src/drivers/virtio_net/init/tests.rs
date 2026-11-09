use super::*;
#[test]
fn modern_ready_preserves_negotiated_features() {
    assert_eq!(driver_ready_status(true), 15);
    assert_eq!(driver_ready_status(false), 7);
}

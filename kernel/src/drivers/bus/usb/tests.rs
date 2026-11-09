use super::endpoint_is_in;

#[test]
fn endpoint_direction_follows_address_bit_7() {
    // OUT endpoints: EP0, EP1, EP15 — host → device.
    assert!(!endpoint_is_in(0x00));
    assert!(!endpoint_is_in(0x01));
    assert!(!endpoint_is_in(0x0F));
    // IN endpoints: EP1..EP15 with the direction bit set — device → host.
    assert!(endpoint_is_in(0x81));
    assert!(endpoint_is_in(0x8F));
    // Only bit 7 matters: the endpoint number must not leak into direction.
    assert!(!endpoint_is_in(0x7F));
}

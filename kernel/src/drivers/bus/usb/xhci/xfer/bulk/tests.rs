use super::ep_ctx_index;

#[test]
fn endpoint_addresses_map_to_direction_specific_contexts() {
    assert_eq!(ep_ctx_index(0x00), 0); // EP0
    assert_eq!(ep_ctx_index(0x01), 1); // EP1 OUT
    assert_eq!(ep_ctx_index(0x81), 2); // EP1 IN
    assert_eq!(ep_ctx_index(0x02), 3); // EP2 OUT
    assert_eq!(ep_ctx_index(0x82), 4); // EP2 IN
    assert_eq!(ep_ctx_index(0x0F), 29); // EP15 OUT
    assert_eq!(ep_ctx_index(0x8F), 30); // EP15 IN
}

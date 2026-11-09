use super::*;
#[test]
fn reserves_prefix_and_final_page_for_packed_framebuffers() {
    assert_eq!(mapping_size(0x7d000, 0x400), Some(0x7e000));
    assert_eq!(mapping_size(0x7d000, 0x600), Some(0x7e000));
    assert_eq!(mapping_size(0x96000, 0x800), Some(0x97000));
    assert_eq!(mapping_size(0x7d000, 0), Some(0x7d000));
    assert_eq!(mapping_size(u64::MAX, 0x800), None);
}

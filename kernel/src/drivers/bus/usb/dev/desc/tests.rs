//! Fixtures for the USB descriptor parsers: a valid 18-byte device
//! descriptor, a valid 25-byte configuration blob, and the negative
//! shapes every parser must reject without panicking on hostile bytes.

use super::*;

/// bDescriptorType of a HID class descriptor — a type the config walk
/// must skip (not one of the brief's published constants).
const DESC_HID: u8 = 0x21;

/// Correct 18-byte device descriptor: max_packet_size0=64, vid=0x044f,
/// pid=0xb10a, one configuration. `pub(crate)`: shared with dev::tests.
pub(crate) fn device_fixture() -> [u8; 18] {
    let mut d = [0u8; 18];
    d[0] = 18; // bLength
    d[1] = DESC_DEVICE;
    d[2] = 0x00; // bcdUSB = 0x0200 (LE)
    d[3] = 0x02;
    d[7] = 64; // bMaxPacketSize0
    d[8] = 0x4f; // idVendor = 0x044f (LE)
    d[9] = 0x04;
    d[10] = 0x0a; // idProduct = 0xb10a (LE)
    d[11] = 0xb1;
    d[17] = 1; // bNumConfigurations
    d
}

/// Correct config blob: 9-byte header + 9-byte HID interface (class 0x03,
/// 1 endpoint) + 7-byte interrupt IN endpoint (address 0x81, attributes 3,
/// max_packet 8, interval 10). wTotalLength = 25. Shared with dev::tests.
pub(crate) fn config_fixture() -> [u8; 25] {
    let mut c = [0u8; 25];
    c[0] = 9; // header bLength
    c[1] = DESC_CONFIGURATION;
    c[2] = 25; // wTotalLength (LE)
    c[4] = 1; // bNumInterfaces
    c[9] = 9; // interface bLength
    c[10] = DESC_INTERFACE;
    c[13] = 1; // bNumEndpoints
    c[14] = CLASS_HID; // bInterfaceClass
    c[15] = 1; // bInterfaceSubClass: boot
    c[16] = 0; // bInterfaceProtocol
    c[18] = 7; // endpoint bLength
    c[19] = DESC_ENDPOINT;
    c[20] = 0x81; // bEndpointAddress: EP1 IN
    c[21] = 3; // bmAttributes: interrupt
    c[22] = 8; // wMaxPacketSize = 8 (LE)
    c[24] = 10; // bInterval
    c
}

#[test]
fn parse_device_valid_fixture_yields_all_fields() {
    assert_eq!(
        parse_device(&device_fixture()),
        Ok(DeviceDesc {
            max_packet_size0: 64,
            vendor: 0x044f,
            product: 0xb10a,
            n_configs: 1,
        })
    );
}

#[test]
fn parse_device_truncated_buffer_is_short() {
    let d = device_fixture();
    assert_eq!(parse_device(&d[..17]), Err(DescError::Short));
    assert_eq!(parse_device(&[]), Err(DescError::Short));
}

#[test]
fn parse_device_zero_blength_is_bad_len() {
    let mut d = device_fixture();
    d[0] = 0;
    assert_eq!(parse_device(&d), Err(DescError::BadLen(0)));
}

#[test]
fn parse_device_wrong_type_is_bad_type() {
    let mut d = device_fixture();
    d[1] = DESC_CONFIGURATION;
    assert_eq!(
        parse_device(&d),
        Err(DescError::BadType(DESC_CONFIGURATION))
    );
}

#[test]
fn parse_config_valid_fixture_walks_blength_chain() {
    let cfg = parse_config(&config_fixture()).unwrap();
    assert_eq!(cfg.n_ifaces, 1);
    let iface = &cfg.interfaces[0];
    assert_eq!(iface.class, CLASS_HID);
    assert_eq!(iface.protocol, 0);
    assert_eq!(iface.n_endpoints, 1);
    assert_eq!(
        iface.endpoints[0],
        EndpointDesc {
            address: 0x81,
            attributes: 3,
            max_packet: 8,
            interval: 10,
        }
    );
}

#[test]
fn parse_config_truncated_header_is_short() {
    let c = config_fixture();
    assert_eq!(parse_config(&c[..8]), Err(DescError::Short));
    assert_eq!(parse_config(&[]), Err(DescError::Short));
}

#[test]
fn parse_config_header_wrong_type_is_bad_type() {
    let mut c = config_fixture();
    c[1] = DESC_DEVICE;
    assert_eq!(parse_config(&c), Err(DescError::BadType(DESC_DEVICE)));
}

#[test]
fn parse_config_total_len_below_header_is_short() {
    let mut c = config_fixture();
    c[2] = 8; // wTotalLength < the 9-byte header
    assert_eq!(parse_config(&c), Err(DescError::Short));
}

#[test]
fn parse_config_total_len_beyond_buffer_is_short() {
    let mut c = config_fixture();
    c[2] = 100; // claims more than the blob holds
    assert_eq!(parse_config(&c), Err(DescError::Short));
}

#[test]
fn parse_config_zero_blength_mid_walk_is_bad_len() {
    let mut c = config_fixture();
    c[2] = 11; // wTotalLength: header + 2 bogus bytes
    c[9] = 0; // bLength=0 would stall the walk
    c[10] = DESC_INTERFACE;
    assert_eq!(parse_config(&c), Err(DescError::BadLen(0)));
}

#[test]
fn parse_config_short_iface_desc_is_bad_len() {
    let mut c = config_fixture();
    c[9] = 5; // interface descriptor claims only 5 bytes
    assert_eq!(parse_config(&c), Err(DescError::BadLen(5)));
}

#[test]
fn parse_config_subdesc_len_past_total_is_bad_len() {
    let mut c = config_fixture();
    c[2] = 20; // wTotalLength cut below what the interface claims
    c[9] = 12; // 12 > 11 remaining
    assert_eq!(parse_config(&c), Err(DescError::BadLen(12)));
}

#[test]
fn parse_config_five_endpoints_one_iface_is_bad_len() {
    // Header + interface + 5 endpoints: the 5th does not fit the bounded array.
    let mut c = [0u8; 9 + 9 + 5 * 7];
    c[0] = 9;
    c[1] = DESC_CONFIGURATION;
    c[2] = c.len() as u8; // wTotalLength = 53 (LE)
    c[4] = 1;
    c[9] = 9;
    c[10] = DESC_INTERFACE;
    c[13] = 5; // bNumEndpoints
    c[14] = CLASS_HID;
    for k in 0..5 {
        let off = 18 + k * 7;
        c[off] = 7;
        c[off + 1] = DESC_ENDPOINT;
        c[off + 2] = 0x81;
        c[off + 3] = 3;
        c[off + 4] = 8;
        c[off + 6] = 10;
    }
    assert_eq!(parse_config(&c), Err(DescError::BadLen(5)));
}

#[test]
fn parse_config_short_endpoint_desc_is_bad_len() {
    let mut c = [0u8; 9 + 9 + 7];
    c[0] = 9;
    c[1] = DESC_CONFIGURATION;
    c[2] = c.len() as u8; // 25 (LE)
    c[9] = 9;
    c[10] = DESC_INTERFACE;
    c[13] = 1;
    c[14] = CLASS_HID;
    c[18] = 3; // endpoint descriptor claims only 3 bytes
    c[19] = DESC_ENDPOINT;
    assert_eq!(parse_config(&c), Err(DescError::BadLen(3)));
}

#[test]
fn parse_config_endpoint_before_iface_is_bad_type() {
    let mut c = [0u8; 9 + 7];
    c[0] = 9;
    c[1] = DESC_CONFIGURATION;
    c[2] = c.len() as u8; // 16 (LE)
    c[9] = 7;
    c[10] = DESC_ENDPOINT; // no interface has been seen
    c[12] = 0x81;
    assert_eq!(parse_config(&c), Err(DescError::BadType(DESC_ENDPOINT)));
}

#[test]
fn parse_config_third_iface_is_bad_len() {
    let mut c = [0u8; 9 + 3 * 9];
    c[0] = 9;
    c[1] = DESC_CONFIGURATION;
    c[2] = c.len() as u8; // 36 (LE)
    for k in 0..3 {
        let off = 9 + k * 9;
        c[off] = 9;
        c[off + 1] = DESC_INTERFACE;
    }
    assert_eq!(parse_config(&c), Err(DescError::BadLen(3)));
}

#[test]
fn parse_config_skips_unknown_descriptor_types() {
    // A HID descriptor (type 0x21) sits between the header and the
    // interface; the walk must skip it by bLength and still find the
    // interface and its endpoint.
    let mut c = [0u8; 9 + 9 + 9 + 7];
    c[0] = 9;
    c[1] = DESC_CONFIGURATION;
    c[2] = c.len() as u8; // 34 (LE)
    c[9] = 9;
    c[10] = DESC_HID;
    c[18] = 9;
    c[19] = DESC_INTERFACE;
    c[23] = CLASS_HID;
    c[27] = 7;
    c[28] = DESC_ENDPOINT;
    c[29] = 0x81;
    c[30] = 3;
    c[31] = 8;
    c[33] = 10;
    let cfg = parse_config(&c).unwrap();
    assert_eq!(cfg.n_ifaces, 1);
    assert_eq!(cfg.interfaces[0].class, CLASS_HID);
    assert_eq!(cfg.interfaces[0].n_endpoints, 1);
    assert_eq!(cfg.interfaces[0].endpoints[0].address, 0x81);
}

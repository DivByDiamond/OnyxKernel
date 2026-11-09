use super::*;

#[test]
fn longest_prefix_lan_route_beats_internet_default() {
    let _serial = crate::net::lock::test_serial();
    let interfaces = [
        Interface {
            ip: [10, 0, 2, 15],
            gateway: [10, 0, 2, 2],
            mask: [255, 255, 255, 0],
            dns: [1, 1, 1, 1],
            enabled: true,
        },
        Interface {
            ip: [192, 168, 50, 1],
            gateway: [0; 4],
            mask: [255, 255, 255, 0],
            dns: [0; 4],
            enabled: true,
        },
    ];
    assert_eq!(choose_route([192, 168, 50, 2], &interfaces, 0), Some(1));
    assert_eq!(choose_route([185, 176, 94, 38], &interfaces, 0), Some(0));
    assert_eq!(choose_route([10, 0, 2, 2], &interfaces, 0), Some(0));
    assert_eq!(choose_route([8, 8, 8, 8], &interfaces, 1), None);
}

#[test]
fn udp_connect_without_route_releases_network_lock() {
    let _serial = crate::net::lock::test_serial();
    // SAFETY: test takes the same global lock as runtime writers and restores config.
    unsafe {
        net_lock();
        let saved = INTERFACES;
        let default = DEFAULT;
        INTERFACES = [EMPTY; 8];
        INTERFACES[0] = Interface {
            ip: [192, 168, 50, 1],
            mask: [255, 255, 255, 0],
            enabled: true,
            ..EMPTY
        };
        DEFAULT = 0;
        let result = super::super::udp::udp_bind_connect([8, 8, 8, 8], 53);
        let depth = super::super::lock::owned_depth();
        INTERFACES = saved;
        DEFAULT = default;
        net_unlock();
        assert_eq!(result, Err(Errno::Io));
        assert_eq!(depth, 1);
    }
}

#[test]
fn accepts_private_static_ip_and_contiguous_mask() {
    let _serial = crate::net::lock::test_serial();
    let mut config = [0; CONFIG_SIZE];
    config[4..8].copy_from_slice(&[192, 168, 50, 1]);
    config[12..16].copy_from_slice(&[255, 255, 255, 0]);
    assert!(validate(&config).is_ok());
    config[12..16].copy_from_slice(&[255, 0, 255, 0]);
    assert!(validate(&config).is_err());
    config[12..16].copy_from_slice(&[255, 255, 255, 0]);
    config[4] = 224;
    assert!(validate(&config).is_err());
}

//! Per-interface IPv4 configuration and longest-prefix routing.
use super::lock::{net_lock, net_unlock};
use crate::drivers::virtio_net;
use onyx_core::errno::{Errno, KResult};

pub const CONFIG_SIZE: usize = 32;
#[derive(Clone, Copy)]
pub struct Interface {
    pub ip: [u8; 4],
    pub gateway: [u8; 4],
    pub mask: [u8; 4],
    pub dns: [u8; 4],
    pub enabled: bool,
}
const EMPTY: Interface = Interface { ip: [0;4], gateway: [0;4], mask: [0;4], dns: [0;4], enabled: false };
static mut INTERFACES: [Interface; 8] = [EMPTY; 8];
static mut DEFAULT: usize = 0;

pub fn choose_route(dst: [u8; 4], interfaces: &[Interface], default: usize) -> Option<usize> {
    let destination = u32::from_be_bytes(dst);
    let direct = interfaces.iter().enumerate().filter(|(_, iface)| {
        let mask = u32::from_be_bytes(iface.mask);
        iface.enabled && mask != 0 && (u32::from_be_bytes(iface.ip) & mask) == (destination & mask)
    }).max_by_key(|(_, iface)| u32::from_be_bytes(iface.mask).count_ones()).map(|(index, _)| index);
    direct.or_else(|| interfaces.get(default).filter(|iface| iface.enabled && iface.gateway != [0;4]).map(|_| default))
}

pub fn validate(data: &[u8; CONFIG_SIZE]) -> KResult<()> {
    let ip = &data[4..8];
    let mask = u32::from_be_bytes(data[12..16].try_into().map_err(|_| Errno::Inval)?);
    if ip[0] == 0 || ip[0] >= 224 || mask == 0 || mask.leading_ones() != mask.count_ones() {
        return Err(Errno::Inval);
    }
    Ok(())
}

/// # Safety
/// Boot-hart-only initialization after the first DHCP exchange.
pub unsafe fn seed_boot_interface() {
    // SAFETY: before secondary harts/processes run; all config buffers initialized.
    unsafe {
        if virtio_net::device_count() > 0 {
            INTERFACES[virtio_net::active_device()] = Interface {
                ip: super::G_IP, gateway: super::G_GW, mask: super::G_MASK, dns: super::G_DNS, enabled: true,
            };
            let _ = crate::srv::event::add_timer(10_000, heartbeat);
        }
    }
}

fn heartbeat() {
    if virtio_net::present() {
        // SAFETY: timer callback runs in kernel context, SIE clear; poll serializes IO.
        unsafe { super::poll(); }
    }
}

pub fn is_configured(index: usize) -> bool {
    // SAFETY: callers hold net_lock or run during single-hart boot.
    unsafe { index < 8 && INTERFACES[index].enabled }
}

/// # Safety
/// Caller holds net_lock, excluding packet IO on another hart.
pub unsafe fn activate(index: usize) -> KResult<()> {
    // SAFETY: driver validates index; its array and config array have the same cap.
    unsafe {
        virtio_net::select_device(index)?;
        if INTERFACES[index].enabled {
            let iface = INTERFACES[index];
            super::G_IP = iface.ip;
            super::G_GW = iface.gateway;
            super::G_MASK = iface.mask;
            super::G_DNS = iface.dns;
        }
        Ok(())
    }
}

/// Run a network operation in one interface's context, restoring the caller's.
/// # Safety
/// Kernel context with interrupts clear; operation must not reconfigure cards.
pub unsafe fn with_interface<T>(index: usize, operation: impl FnOnce() -> KResult<T>) -> KResult<T> {
    // SAFETY: recursive network lock serializes both context switches and IO.
    unsafe {
        net_lock();
        let previous = virtio_net::active_device();
        let result = match activate(index) { Ok(()) => operation(), Err(error) => Err(error) };
        let _ = activate(previous);
        net_unlock();
        result
    }
}

pub fn dns_server() -> [u8; 4] {
    // SAFETY: the configured default interface is read under the network lock.
    unsafe { net_lock(); let dns = INTERFACES[DEFAULT].dns; net_unlock(); dns }
}

pub fn route(dst: [u8; 4]) -> Option<usize> {
    // SAFETY: called under net_lock; DHCP before configuration uses the first card.
    unsafe {
        if dst == [255;4] || !INTERFACES.iter().any(|iface| iface.enabled) {
            return Some(virtio_net::active_device());
        }
        choose_route(dst, &INTERFACES, DEFAULT)
    }
}

/// # Safety
/// Privileged syscall; all packet IO is serialized while addresses change.
pub unsafe fn apply(data: &[u8; CONFIG_SIZE]) -> KResult<()> {
    validate(data)?;
    let index = u32::from_le_bytes(data[..4].try_into().map_err(|_| Errno::Inval)?) as usize;
    // SAFETY: bounded interface table, serialized by the recursive network lock.
    unsafe {
        net_lock();
        if index >= virtio_net::device_count() || index >= 8 { net_unlock(); return Err(Errno::Inval); }
        super::tcp::clear_interface(index);
        super::udp::clear_interface(index);
        super::eth::clear_arp_cache();
        INTERFACES[index] = Interface {
            ip: data[4..8].try_into().map_err(|_| Errno::Inval)?,
            gateway: data[8..12].try_into().map_err(|_| Errno::Inval)?,
            mask: data[12..16].try_into().map_err(|_| Errno::Inval)?,
            dns: data[16..20].try_into().map_err(|_| Errno::Inval)?,
            enabled: true,
        };
        if INTERFACES[index].gateway != [0;4] { DEFAULT = index; }
        let result = activate(index);
        net_unlock();
        result
    }
}

pub fn snapshot(index: Option<usize>) -> KResult<[u8; CONFIG_SIZE]> {
    // SAFETY: all runtime config writers take the same lock; index checked.
    unsafe {
        net_lock();
        let index = index.unwrap_or(virtio_net::active_device());
        if index >= virtio_net::device_count() || index >= 8 { net_unlock(); return Err(Errno::Inval); }
        let iface = INTERFACES[index];
        let mut data = [0; CONFIG_SIZE];
        data[..4].copy_from_slice(&(index as u32).to_le_bytes());
        data[4..8].copy_from_slice(&iface.ip);
        data[8..12].copy_from_slice(&iface.gateway);
        data[12..16].copy_from_slice(&iface.mask);
        data[16..20].copy_from_slice(&iface.dns);
        data[20..26].copy_from_slice(&virtio_net::device_mac(index)?);
        data[26] = u8::from(iface.enabled);
        data[27] = DEFAULT as u8;
        data[28..32].copy_from_slice(&(virtio_net::device_count() as u32).to_le_bytes());
        net_unlock();
        Ok(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn longest_prefix_lan_route_beats_internet_default() {
        let _serial = crate::net::lock::test_serial();
        let interfaces = [
            Interface { ip: [10, 0, 2, 15], gateway: [10, 0, 2, 2], mask: [255, 255, 255, 0], dns: [1,1,1,1], enabled: true },
            Interface { ip: [192, 168, 50, 1], gateway: [0;4], mask: [255,255,255,0], dns: [0;4], enabled: true },
        ];
        assert_eq!(choose_route([192,168,50,2], &interfaces, 0), Some(1));
        assert_eq!(choose_route([185,176,94,38], &interfaces, 0), Some(0));
        assert_eq!(choose_route([10,0,2,2], &interfaces, 0), Some(0));
        assert_eq!(choose_route([8,8,8,8], &interfaces, 1), None);
    }

    #[test]
    fn udp_connect_without_route_releases_network_lock() {
        let _serial = crate::net::lock::test_serial();
        // SAFETY: test takes the same global lock as runtime writers and restores config.
        unsafe {
            net_lock();
            let saved = INTERFACES;
            let default = DEFAULT;
            INTERFACES = [EMPTY;8];
            INTERFACES[0] = Interface {ip:[192,168,50,1],mask:[255,255,255,0],enabled:true,..EMPTY};
            DEFAULT = 0;
            let result = super::super::udp::udp_bind_connect([8,8,8,8],53);
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
}

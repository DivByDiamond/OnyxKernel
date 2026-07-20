pub mod dhcp;
pub mod dns;
pub mod eth;
pub mod ip;
pub mod tcp;

#[cfg(test)]
mod tests;
pub mod udp;

pub use dns::dns_resolve;
pub use tcp::{tcp_close, tcp_connect, tcp_recv, tcp_send};
pub use udp::{udp_bind, udp_close, udp_recv, udp_send, udp_sendto};

use crate::drivers::virtio_net;

pub static mut G_IP: [u8; 4] = [0; 4];
pub static mut G_GW: [u8; 4] = [0; 4];
pub static mut G_MASK: [u8; 4] = [0; 4];
pub static mut G_DNS: [u8; 4] = [0; 4];

pub unsafe fn init(ip: [u8; 4], gateway: [u8; 4], netmask: [u8; 4]) {
    G_IP = ip;
    G_GW = gateway;
    G_MASK = netmask;
}

pub unsafe fn poll() {
    loop {
        let mut buf = [0u8; 2048];
        match virtio_net::xfer::recv_into(&mut buf) {
            Ok(n) => {
                if n >= 14 {
                    eth::dispatch(&buf[..n]);
                }
            }
            Err(_) => break,
        }
    }
}

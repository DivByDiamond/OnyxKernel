pub mod rx;
pub mod datagram;
pub mod sock;

pub use rx::handle_udp;
pub use sock::{
    udp_bind, udp_bind_any, udp_send_from, udp_bind_connect, udp_close, udp_recv, udp_recv_from, udp_send, udp_send_bound, udp_sendto,
};

pub const UDP_HLEN: usize = 8;
pub const MAX_UDP_SOCKS: usize = 8;
pub const UDP_BUF_SIZE: usize = 2048;

#[derive(Clone, Copy)]
pub struct UdpSocket {
    pub owner_pid:u32,
    pub iface: usize,
    pub local_port: u16,
    pub remote_ip: [u8; 4],
    pub remote_port: u16,
    pub bound: bool,
    pub connected: bool,
    pub datagrams: datagram::DatagramQueue,
}

pub(super) static mut UDP_SOCKS: [Option<UdpSocket>; MAX_UDP_SOCKS] = [const { None }; MAX_UDP_SOCKS];
static mut NEXT_UDP_PORT: u16 = 50000;

fn alloc_udp_sock() -> Option<usize> {
    // SAFETY: read-only scan of UDP_SOCKS; every slot is always initialized (const { None }).
    unsafe { UDP_SOCKS.iter().position(Option::is_none) }
}

fn next_udp_port() -> u16 {
    // SAFETY: RMW on the NEXT_UDP_PORT static; runs under net_lock() held
    // by every caller (net sync fix, todo P1 #4).
    unsafe {
        let p = NEXT_UDP_PORT;
        NEXT_UDP_PORT = NEXT_UDP_PORT.wrapping_add(1);
        p
    }
}

/// # Safety
/// Caller holds net_lock; interface reconfiguration invalidates old sockets.
pub unsafe fn clear_interface(index: usize) {
    // SAFETY: socket table is exclusively accessed under the network lock.
    unsafe {
        for slot in UDP_SOCKS.iter_mut() {
            if slot.as_ref().is_some_and(|socket| socket.iface == index) { *slot = None; }
        }
    }
}

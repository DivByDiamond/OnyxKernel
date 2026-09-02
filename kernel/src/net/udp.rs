pub mod rx;
pub mod sock;

pub use rx::handle_udp;
pub use sock::{
    udp_bind, udp_bind_connect, udp_close, udp_recv, udp_send, udp_send_bound, udp_sendto,
};

pub const UDP_HLEN: usize = 8;
pub const MAX_UDP_SOCKS: usize = 8;
pub const UDP_BUF_SIZE: usize = 2048;

#[derive(Clone, Copy)]
pub struct UdpSocket {
    pub local_port: u16,
    pub remote_ip: [u8; 4],
    pub remote_port: u16,
    pub bound: bool,
    pub connected: bool,
    pub recv_buf: [u8; UDP_BUF_SIZE],
    pub recv_len: usize,
    pub recv_head: usize,
}

static mut UDP_SOCKS: [Option<UdpSocket>; MAX_UDP_SOCKS] = [const { None }; MAX_UDP_SOCKS];
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

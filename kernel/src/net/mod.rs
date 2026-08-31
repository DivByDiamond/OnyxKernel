pub mod dhcp;
pub mod dns;
pub mod eth;
pub mod ip;
pub mod lock;
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

/// # Safety
///
/// Boot-time configuration: must run on the boot hart before secondary
/// harts are released (kmain does this before `launch()`), so the config
/// statics are written without concurrent access; afterwards they are
/// only read.
pub unsafe fn init(ip: [u8; 4], gateway: [u8; 4], netmask: [u8; 4]) {
    // SAFETY: boot-hart-only write of the config statics, before any other hart runs.
    unsafe {
        G_IP = ip;
        G_GW = gateway;
        G_MASK = netmask;
    }
}

/// Net-stack serialization (todo P1 #4, #6).
///
/// Drives the RX path under the recursive NET lock. All net table state
/// (UDP_SOCKS, CONNS, ARP cache, IP_ID, ephemeral ports) and the
/// virtio-net TX/RX ring accesses are now protected by this single lock;
/// see `net::lock` for why it must be RECURSIVE:
///
/// * TX: `udp_send` → `ip::send_packet` → (ARP miss) → `arp_request` +
///   `net::poll` — a poll runs INSIDE a send, and the poll's dispatch
///   handlers (`handle_udp`/`handle_tcp`/`handle_arp`) mutate the same
///   tables again. A plain spinlock would self-deadlock on the second
///   acquisition.
/// * RX replies: `poll` → `handle_icmp` → `send_frame` — same nesting.
///
/// # Safety
///
/// Must run in kernel context with SIE clear (SpinLock invariant); may
/// now be called from syscall paths on any hart — the lock makes concurrent
/// callers safe, they simply serialize.
pub unsafe fn poll() {
    // SAFETY: the whole drain loop (tcp::tick table sweep, recv_into ring
    // access, dispatch into the protocol handlers) runs under net_lock();
    // nested acquisitions from the handlers are recursive (same hart) and
    // therefore deadlock-free.
    unsafe {
        lock::net_lock();
        // Reclaim expired TIMEWAIT TCP slots even when no packets arrive.
        tcp::tick();
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
        lock::net_unlock();
    }
}

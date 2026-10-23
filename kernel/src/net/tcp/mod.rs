// pub(crate): host unit-tests (net/tests.rs) exercise tcp_checksum /
// tcp_checksum_ok directly.
pub(crate) mod conn;
mod handle;
pub mod listen;
mod sock;
mod state;
mod retry;
#[cfg(test)]
mod tests;

pub use handle::handle_tcp;
pub use sock::{tcp_close, tcp_connect, tcp_recv, tcp_send};

/// Periodic TCP maintenance: reclaim TIMEWAIT slots. Called from
/// net::poll so dead connections cannot accumulate.
pub fn tick() {
    conn::sweep_timewait(conn::now_us());
    // SAFETY: tick is called only by net::poll while the network lock is held.
    unsafe { retry::tick(conn::now_us()); }
}

/// # Safety
/// Caller holds net_lock; interface reconfiguration invalidates old sockets.
pub unsafe fn clear_interface(index: usize) {
    // SAFETY: socket table is exclusively accessed under the network lock.
    unsafe {
        for slot in conn::CONNS.iter_mut() {
            if slot.as_ref().is_some_and(|socket| socket.iface == index) { *slot = None; }
        }
    }
}

// pub(crate): host unit-tests (net/tests.rs) exercise tcp_checksum /
// tcp_checksum_ok directly.
pub(crate) mod conn;
mod handle;
mod sock;
mod state;
#[cfg(test)]
mod tests;

pub use handle::handle_tcp;
pub use sock::{tcp_close, tcp_connect, tcp_recv, tcp_send};

/// Periodic TCP maintenance: reclaim TIMEWAIT slots. Called from
/// net::poll so dead connections cannot accumulate.
pub fn tick() {
    conn::sweep_timewait(conn::now_us());
}

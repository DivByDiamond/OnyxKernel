//! UDP socket operations, split by responsibility.
//!
//! `bind` — slot allocation, connection setup, and teardown.
//! `io`   — TX/RX on an allocated slot.

mod bind;
mod io;

pub use bind::{udp_bind, udp_bind_any, udp_bind_connect, udp_close};
pub use io::{udp_recv, udp_recv_from, udp_send, udp_send_bound, udp_send_from, udp_sendto};

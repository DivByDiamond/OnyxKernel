//! IPC channel ring buffer: send/recv over a lock-protected ring, with
//! sleep/wake lists for blocking waiters. Split by responsibility:
//! [`access`] (pid permission checks) and [`flow`] (the two data-path
//! operations plus sleep/wake list management).

mod access;
mod flow;

pub use flow::{disconnect_waiter, recv, send};
pub use onyx_core::ringbuf::{ring_free, ring_read, ring_used, ring_write};

#[cfg(test)]
mod tests;

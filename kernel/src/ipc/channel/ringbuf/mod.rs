//! IPC channel ring buffer: send/recv over a lock-protected ring, with
//! sleep/wake lists for blocking waiters. Split by responsibility:
//! `access` (pid permission checks), `wait` (sleep/wake list management),
//! `send`/`recv` (the two data-path operations).

mod access;
mod recv;
mod send;
mod wait;

pub use onyx_core::ringbuf::{ring_free, ring_read, ring_used, ring_write};
pub use recv::recv;
pub use send::send;
pub use wait::disconnect_waiter;

#[cfg(test)]
mod tests;

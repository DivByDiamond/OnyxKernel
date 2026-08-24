//! IPC channels — create/connect/send/recv over a lock-protected ring buffer.
mod chan;
pub mod ringbuf;
pub mod types;
pub use chan::*;
pub use ringbuf::*;
pub use types::*;
